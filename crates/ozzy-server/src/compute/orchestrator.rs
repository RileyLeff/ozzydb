//! DAG orchestrator: runs jobs by executing nodes in parallel waves.
//!
//! Given a job, the orchestrator:
//! 1. Loads context from DB (commit state, endpoint, transforms, environments)
//! 2. Computes topological waves (groups of nodes that can run in parallel)
//! 3. For each wave: check cache, dispatch uncached nodes in parallel
//! 4. Updates job/node status as execution progresses
//! 5. Stores output and sets job result on completion

use std::collections::{HashMap, HashSet};

use uuid::Uuid;

use crate::AppState;

/// Run a job to completion: load context, execute DAG in parallel waves, update status.
pub async fn run_job(state: AppState, job_id: Uuid) {
    if let Err(e) = run_job_inner(&state, job_id).await {
        tracing::error!("Job {} failed: {}", job_id, e);
        if let Err(db_err) = state.db.set_job_error(job_id, &e.to_string()).await {
            tracing::error!("Job {}: failed to record error: {}", job_id, db_err);
        }
    }
}

async fn run_job_inner(state: &AppState, job_id: Uuid) -> Result<(), anyhow::Error> {
    let job = state
        .db
        .get_job(job_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Job {} not found", job_id))?;

    state.db.update_job_status(job_id, "running").await?;

    // Load context from DB
    let project = state
        .db
        .get_project_by_id(job.project_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Project {} not found", job.project_id))?;

    let commit = state
        .db
        .get_commit_by_id(job.commit_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Commit {} not found", job.commit_id))?;

    let commit_state = state
        .db
        .get_commit_state(commit.id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Commit state missing for commit {}", commit.id))?;

    let endpoints: HashMap<String, ozzy_core::toml_spec::EndpointDef> =
        serde_json::from_value(commit_state.endpoints.clone())?;

    let endpoint_def = endpoints
        .get(&job.endpoint_name)
        .ok_or_else(|| anyhow::anyhow!("Endpoint '{}' not found", job.endpoint_name))?;

    let transforms: HashMap<String, ozzy_core::toml_spec::TransformDef> =
        serde_json::from_value(commit_state.transforms.clone())?;

    let environments: HashMap<String, ozzy_core::toml_spec::EnvironmentDef> =
        serde_json::from_value(commit_state.environments.clone())?;

    let resolved_params: serde_json::Value = job.params.clone();
    let source_dir = super::super::api::v1::fetch::retrieve_source_code(state, &commit).await;
    let edge_map = super::super::api::v1::fetch::build_edge_map(endpoint_def);
    let platform = ozzy_core::platform::PlatformFingerprint::detect();
    let platform_hash = platform.hash();

    // Compute execution waves (groups of nodes that can run in parallel)
    let waves = compute_waves(endpoint_def)?;
    tracing::info!(
        "Job {}: {} waves, {} total nodes",
        job_id,
        waves.len(),
        waves.iter().map(|w| w.len()).sum::<usize>()
    );

    let mut node_outputs: HashMap<String, NodeOutput> = HashMap::new();

    for (wave_idx, wave) in waves.iter().enumerate() {
        tracing::debug!(
            "Job {}: executing wave {} ({} nodes: {:?})",
            job_id,
            wave_idx,
            wave.len(),
            wave
        );

        // Execute all nodes in this wave concurrently
        let mut handles = Vec::new();

        for node_name in wave {
            let state = state.clone();
            let node_name = node_name.clone();
            let job_endpoint_name = job.endpoint_name.clone();
            let commit = commit.clone();
            let project_id = project.id;
            let resolved_params = resolved_params.clone();
            let platform_hash = platform_hash.clone();
            let platform = platform.clone();
            let transforms = transforms.clone();
            let environments = environments.clone();
            let endpoint_def = endpoint_def.clone();
            let edge_map_for_node: Vec<(String, String)> = edge_map
                .get(node_name.as_str())
                .cloned()
                .unwrap_or_default()
                .iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect();

            // Gather input hashes from previous waves' outputs
            let mut input_hashes_snapshot: Vec<(String, String)> = Vec::new();
            for (input_name, source) in &edge_map_for_node {
                if let Some(output) = node_outputs.get(source.as_str()) {
                    input_hashes_snapshot.push((input_name.clone(), output.output_hash.clone()));
                }
                // Data/collection sources are resolved inside the task
            }

            let source_dir_path = source_dir.as_ref().map(|d| d.path().to_path_buf());
            let node_outputs_snapshot: HashMap<String, NodeOutput> = node_outputs.clone();

            handles.push(tokio::spawn(async move {
                let result = execute_node(
                    &state,
                    job_id,
                    &node_name,
                    &endpoint_def,
                    &transforms,
                    &environments,
                    &commit,
                    project_id,
                    &job_endpoint_name,
                    &resolved_params,
                    &platform_hash,
                    &platform,
                    &edge_map_for_node,
                    &node_outputs_snapshot,
                    source_dir_path.as_deref(),
                )
                .await;
                (node_name, result)
            }));
        }

        // Await all nodes in this wave
        for handle in handles {
            let (node_name, result) = handle
                .await
                .map_err(|e| anyhow::anyhow!("Node execution task panicked: {}", e))?;

            match result {
                Ok(output) => {
                    node_outputs.insert(node_name, output);
                }
                Err(e) => {
                    return Err(anyhow::anyhow!("Node '{}' failed: {}", node_name, e));
                }
            }
        }
    }

    // Set job output from terminal node
    let final_node = super::super::api::v1::fetch::find_terminal_node_anyhow(endpoint_def)?;
    let final_output = node_outputs
        .get(final_node)
        .ok_or_else(|| anyhow::anyhow!("Final node '{}' has no output", final_node))?;

    state
        .db
        .set_job_output(
            job_id,
            &final_output.output_hash,
            &final_output.content_type,
        )
        .await?;

    tracing::info!("Job {} completed successfully", job_id);
    Ok(())
}

/// Execute a single node: check cache, run if uncached, store output.
async fn execute_node(
    state: &AppState,
    job_id: Uuid,
    node_name: &str,
    endpoint_def: &ozzy_core::toml_spec::EndpointDef,
    transforms: &HashMap<String, ozzy_core::toml_spec::TransformDef>,
    environments: &HashMap<String, ozzy_core::toml_spec::EnvironmentDef>,
    commit: &crate::db::Commit,
    project_id: Uuid,
    endpoint_name: &str,
    resolved_params: &serde_json::Value,
    platform_hash: &str,
    platform: &ozzy_core::platform::PlatformFingerprint,
    edges_for_node: &[(String, String)],
    node_outputs: &HashMap<String, NodeOutput>,
    source_dir: Option<&std::path::Path>,
) -> Result<NodeOutput, anyhow::Error> {
    state
        .db
        .update_node_status(job_id, node_name, "running")
        .await?;

    let node_def = endpoint_def
        .nodes
        .get(node_name)
        .ok_or_else(|| anyhow::anyhow!("Node '{}' missing from endpoint", node_name))?;

    let transform_def = transforms.get(&node_def.transform).ok_or_else(|| {
        anyhow::anyhow!(
            "Transform '{}' not found for node '{}'",
            node_def.transform,
            node_name
        )
    })?;

    // Resolve inputs
    let mut input_hashes: Vec<(String, String)> = Vec::new();
    for (input_name, source) in edges_for_node {
        let hash = resolve_edge_source(source, state, project_id, node_outputs).await?;
        input_hashes.push((input_name.clone(), hash));
    }

    // Resolve params
    let node_params = super::super::api::v1::fetch::resolve_node_params(
        node_name,
        node_def,
        endpoint_def,
        resolved_params,
    );
    let params_hash = ozzy_core::hash::blake3_hash(
        serde_json::to_string(&node_params)
            .unwrap_or_default()
            .as_bytes(),
    );

    // Resolve secrets hash
    let secrets_hash = resolve_secrets_hash(state, project_id, transform_def).await?;

    // Resolve environment
    let env_def = environments
        .get(&transform_def.environment)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Environment '{}' not found for transform '{}'",
                transform_def.environment,
                node_def.transform,
            )
        })?;

    let (env_image, env_hash, lockfile_hash) =
        super::super::api::v1::fetch::resolve_environment_image_anyhow(
            state,
            env_def,
            &commit.git_repo,
            &commit.git_commit_sha,
        )
        .await?;

    // Compute source hash
    let (source_hash, function_name) =
        compute_source_hash(transform_def, node_def, commit, source_dir)?;

    let params_schema_hash =
        super::super::api::v1::fetch::compute_params_schema_hash(transform_def);

    let transform_hash = ozzy_core::hash::transform_hash(
        &source_hash,
        &function_name,
        &lockfile_hash,
        &env_hash,
        &params_schema_hash,
    );

    let input_refs: Vec<(&str, &str)> = input_hashes
        .iter()
        .map(|(name, hash)| (name.as_str(), hash.as_str()))
        .collect();

    let mat_hash = ozzy_core::hash::materialized_hash(
        &input_refs,
        &transform_hash,
        &params_hash,
        platform_hash,
        secrets_hash.as_deref(),
    );

    // Check materialized cache
    if let Some(cached) = state.db.get_materialized_cache(&mat_hash).await? {
        state.db.touch_materialized_cache(&mat_hash).await?;
        tracing::info!(
            "Job {}: cache hit for node '{}': {}",
            job_id,
            node_name,
            mat_hash.get(..12).unwrap_or(&mat_hash)
        );
        state
            .db
            .update_node_status(job_id, node_name, "done")
            .await?;
        return Ok(NodeOutput {
            materialized_hash: mat_hash,
            output_hash: cached.output_hash,
            content_type: cached.output_content_type,
            byte_size: cached.output_byte_size,
            cache_hit: true,
        });
    }

    // Execute uncached node
    let backend = state
        .compute
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Compute is not enabled on this server"))?;

    let env_image_ref = env_image
        .as_ref()
        .map(|img| img.image_ref.clone())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Environment '{}' has not been built yet",
                transform_def.environment
            )
        })?;

    if env_image.as_ref().and_then(|img| img.built_at).is_none() {
        anyhow::bail!(
            "Environment '{}' is still building",
            transform_def.environment
        );
    }

    // Generate runner script
    let runner_script = if let Some(source) = &transform_def.source {
        let (file_path, func_name) = crate::runners::validate_source_ref(source).map_err(|e| {
            anyhow::anyhow!(
                "Invalid source reference '{}' in transform '{}': {}",
                source,
                node_def.transform,
                e
            )
        })?;
        let runner_type = crate::runners::detect_runner_type(source)
            .ok_or_else(|| anyhow::anyhow!("Unsupported source file type in '{}'", source))?;
        match runner_type {
            crate::runners::RunnerType::Python => {
                crate::runners::python::generate(file_path, func_name)
            }
            crate::runners::RunnerType::R => crate::runners::r::generate(file_path, func_name),
            crate::runners::RunnerType::Command => {
                anyhow::bail!("Source-based transform incorrectly detected as Command type");
            }
        }
    } else if let Some(command) = &transform_def.command {
        let input_names: Vec<&str> = transform_def.inputs.keys().map(|s| s.as_str()).collect();
        crate::runners::command::generate_shell_wrapper(command, &input_names)
    } else {
        anyhow::bail!(
            "Transform '{}' has neither source nor command",
            node_def.transform,
        );
    };

    let runner_ext = if transform_def.source.is_some() {
        let rt = crate::runners::detect_runner_type(transform_def.source.as_deref().unwrap_or(""))
            .unwrap_or(crate::runners::RunnerType::Python);
        match rt {
            crate::runners::RunnerType::Python => "py",
            crate::runners::RunnerType::R => "R",
            crate::runners::RunnerType::Command => "sh",
        }
    } else {
        "sh"
    };

    let runner_type = if transform_def.source.is_some() {
        crate::runners::detect_runner_type(transform_def.source.as_deref().unwrap_or(""))
            .unwrap_or(crate::runners::RunnerType::Python)
    } else {
        crate::runners::RunnerType::Command
    };

    let is_fly = state
        .compute
        .as_ref()
        .map(|c| c.is_fly())
        .unwrap_or(false);
    let init_script = if is_fly {
        crate::runners::init::generate_fly_init(runner_type)
    } else {
        crate::runners::init::generate_docker_init(runner_type)
    };

    // Build input manifest and env vars
    let compute_inputs: Vec<crate::compute::InputSpec> = Vec::new();
    let input_manifest = crate::compute::docker::build_input_manifest(&compute_inputs);
    let param_env_vars = crate::compute::docker::build_param_env_vars(&node_params);

    let mut env_vars: HashMap<String, String> = HashMap::new();
    env_vars.insert(
        "OZZY_PARAMS".to_string(),
        serde_json::to_string(&node_params).unwrap_or_default(),
    );
    env_vars.insert(
        "OZZY_INPUT_MANIFEST".to_string(),
        serde_json::to_string(&input_manifest).unwrap_or_default(),
    );
    for (key, value) in param_env_vars {
        env_vars.insert(key, value);
    }

    // Inject secrets
    const RESERVED_SECRET_NAMES: &[&str] = &[
        "PATH",
        "HOME",
        "PYTHONHASHSEED",
        "OMP_NUM_THREADS",
        "MKL_NUM_THREADS",
        "OPENBLAS_NUM_THREADS",
        "NUMEXPR_NUM_THREADS",
        "VECLIB_MAXIMUM_THREADS",
        "PYTHONDONTWRITEBYTECODE",
        "PYTHONUNBUFFERED",
    ];
    if !transform_def.secrets.is_empty() {
        let enc_key = state
            .config
            .secrets_encryption_key
            .as_ref()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Transform '{}' requires secrets but the server has no secrets encryption key",
                    node_def.transform
                )
            })?;
        for secret_name in &transform_def.secrets {
            if secret_name.starts_with("OZZY_") {
                anyhow::bail!("Secret '{}' uses reserved prefix 'OZZY_'", secret_name);
            }
            if RESERVED_SECRET_NAMES
                .iter()
                .any(|&r| r.eq_ignore_ascii_case(secret_name))
            {
                anyhow::bail!(
                    "Secret '{}' would override a reserved runtime environment variable",
                    secret_name
                );
            }
            let secret = state
                .db
                .get_secret(project_id, secret_name)
                .await?
                .ok_or_else(|| {
                    anyhow::anyhow!("Required secret '{}' not found", secret_name)
                })?;
            let decrypted =
                super::super::api::v1::fetch::decrypt_secret(&secret.encrypted_value, enc_key)?;
            env_vars.insert(secret_name.clone(), decrypted);
        }
    }

    // Execute via Docker
    let compute_request = crate::compute::ComputeRequest {
        image: env_image_ref,
        runner_script,
        runner_ext: runner_ext.to_string(),
        init_script,
        inputs: compute_inputs,
        env_vars,
        timeout_secs: state.config.compute.timeout_secs,
        memory_limit: Some(state.config.compute.memory_limit.clone()),
        cpu_limit: Some(state.config.compute.cpu_limit.clone()),
        network: transform_def.network,
        runtime: state.config.compute.docker_runtime.clone(),
        source_dir: source_dir.map(|p| p.to_path_buf()),
    };

    let result = backend
        .run(&compute_request)
        .await
        .map_err(|e| anyhow::anyhow!("Compute execution failed: {}", e))?;

    if !result.success() {
        let logs = result.logs.clone();
        let exit_code = result.exit_code;
        result.cleanup().await;
        anyhow::bail!(
            "Transform '{}' failed (exit {}): {}",
            node_def.transform,
            exit_code,
            logs
        );
    }

    // Read output
    let compute_duration_ms = result.duration_ms;
    let read_result: Result<(Vec<u8>, String), anyhow::Error> = async {
        let output_files =
            super::super::api::v1::fetch::list_output_files_anyhow(&result.output_dir).await?;
        let primary_output = super::super::api::v1::fetch::find_primary_output(&output_files)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Transform '{}' produced no output files",
                    node_def.transform
                )
            })?;
        let content_type = super::super::api::v1::fetch::infer_output_content_type(primary_output);
        let bytes = tokio::fs::read(primary_output).await?;
        Ok((bytes, content_type))
    }
    .await;
    result.cleanup().await;
    let (output_bytes, output_content_type) = read_result?;
    let output_hash = ozzy_core::hash::blake3_hash(&output_bytes);
    let output_byte_size = output_bytes.len() as i64;
    let output_ext = super::super::api::v1::fetch::content_type_to_extension(&output_content_type);

    // Store output (use store_with_hash to avoid redundant blake3 computation)
    state
        .storage
        .store_with_hash(&output_hash, &output_bytes, &output_ext)
        .await?;
    let output_r2_key = state.storage.storage_key(&output_hash, &output_ext)?;

    // Insert materialized cache record
    let platform_str = serde_json::to_string(platform).unwrap_or_default();
    state
        .db
        .insert_materialized_cache(
            &mat_hash,
            project_id,
            commit.id,
            endpoint_name,
            node_name,
            &node_def.transform,
            &output_hash,
            &output_r2_key,
            &output_content_type,
            output_byte_size,
            &platform_str,
            1,
        )
        .await?;

    tracing::info!(
        "Job {}: computed node '{}' ({}ms): {}",
        job_id,
        node_name,
        compute_duration_ms,
        mat_hash.get(..12).unwrap_or(&mat_hash)
    );

    state
        .db
        .update_node_status(job_id, node_name, "done")
        .await?;

    Ok(NodeOutput {
        materialized_hash: mat_hash,
        output_hash,
        content_type: output_content_type,
        byte_size: output_byte_size,
        cache_hit: false,
    })
}

// ── Helpers ──────────────────────────────────────────────────────

/// Node output metadata.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct NodeOutput {
    pub materialized_hash: String,
    pub output_hash: String,
    pub content_type: String,
    pub byte_size: i64,
    pub cache_hit: bool,
}

/// Compute execution waves: groups of nodes whose dependencies are all in earlier waves.
///
/// Wave 0 = nodes with no node dependencies (only data/collection inputs).
/// Wave N = nodes whose node dependencies are all in waves 0..N-1.
pub fn compute_waves(
    endpoint: &ozzy_core::toml_spec::EndpointDef,
) -> Result<Vec<Vec<String>>, anyhow::Error> {
    // Build node dependency graph (only node→node, not data/collection sources)
    let node_names: HashSet<&str> = endpoint.nodes.keys().map(|s| s.as_str()).collect();
    let mut deps: HashMap<&str, HashSet<&str>> = HashMap::new();

    for name in &node_names {
        deps.insert(name, HashSet::new());
    }

    for edge in &endpoint.edges {
        let target_node = edge.to.split('.').next().unwrap_or(&edge.to);
        if !node_names.contains(target_node) {
            continue;
        }

        let source = &edge.from;
        // If the source is a node name (not data: or collection: or endpoint:)
        let source_node = source.split('.').next().unwrap_or(source);
        if node_names.contains(source_node) {
            deps.get_mut(target_node).unwrap().insert(source_node);
        }
    }

    let mut waves = Vec::new();
    let mut assigned: HashSet<&str> = HashSet::new();

    loop {
        let mut wave: Vec<String> = deps
            .iter()
            .filter(|(name, node_deps)| {
                !assigned.contains(**name) && node_deps.iter().all(|d| assigned.contains(d))
            })
            .map(|(name, _)| name.to_string())
            .collect();
        wave.sort();

        if wave.is_empty() {
            if assigned.len() < node_names.len() {
                anyhow::bail!("Cycle detected in endpoint DAG");
            }
            break;
        }

        for name in &wave {
            assigned.insert(deps.keys().find(|k| **k == name.as_str()).unwrap());
        }

        waves.push(wave);
    }

    Ok(waves)
}

/// Resolve an edge source to a content hash.
async fn resolve_edge_source(
    source: &str,
    state: &AppState,
    project_id: Uuid,
    node_outputs: &HashMap<String, NodeOutput>,
) -> Result<String, anyhow::Error> {
    if let Some(data_name) = source.strip_prefix("data:") {
        let atom = state
            .db
            .get_data_atom(project_id, data_name)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Data atom '{}' not found", data_name))?;
        if atom.yanked {
            anyhow::bail!("Data atom '{}' has been yanked", data_name);
        }
        Ok(atom.hash)
    } else if let Some(coll_name) = source.strip_prefix("collection:") {
        let collection = state
            .db
            .get_collection(project_id, coll_name)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Collection '{}' not found", coll_name))?;
        if collection.yanked {
            anyhow::bail!("Collection '{}' has been yanked", coll_name);
        }
        let version = state
            .db
            .get_latest_collection_version(collection.id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Collection '{}' has no versions", coll_name))?;
        Ok(version.hash)
    } else if source.starts_with("endpoint:") {
        Ok(ozzy_core::hash::blake3_hash(source.as_bytes()))
    } else {
        let output = node_outputs.get(source).ok_or_else(|| {
            anyhow::anyhow!(
                "Node '{}' output not available (execution order issue?)",
                source
            )
        })?;
        Ok(output.output_hash.clone())
    }
}

/// Compute source hash for a transform (orchestrator version).
fn compute_source_hash(
    transform_def: &ozzy_core::toml_spec::TransformDef,
    node_def: &ozzy_core::toml_spec::NodeDef,
    commit: &crate::db::Commit,
    source_dir: Option<&std::path::Path>,
) -> Result<(String, String), anyhow::Error> {
    if let Some(source) = &transform_def.source {
        let (_file_path, func) = crate::runners::parse_source_ref(source).ok_or_else(|| {
            anyhow::anyhow!(
                "Invalid source ref '{}' for transform '{}'",
                source,
                node_def.transform
            )
        })?;
        let file_path_str = source.split(':').next().unwrap_or(source);
        let hash = if let Some(sd) = source_dir {
            let full_path = sd.join(file_path_str);
            let canonical = full_path.canonicalize().ok();
            let within_source = canonical
                .as_ref()
                .and_then(|c| sd.canonicalize().ok().map(|sdc| c.starts_with(sdc)))
                .unwrap_or(false);
            if !within_source {
                anyhow::bail!("Source path '{}' escapes source directory", file_path_str);
            }
            std::fs::read(&full_path)
                .map(|bytes| ozzy_core::hash::blake3_hash(&bytes))
                .unwrap_or_else(|_| {
                    ozzy_core::hash::blake3_hash(
                        format!("{}:{}", node_def.transform, commit.git_commit_sha).as_bytes(),
                    )
                })
        } else {
            ozzy_core::hash::blake3_hash(
                format!("{}:{}", node_def.transform, commit.git_commit_sha).as_bytes(),
            )
        };
        Ok((hash, func.to_owned()))
    } else if let Some(command) = &transform_def.command {
        Ok((
            ozzy_core::hash::blake3_hash(command.as_bytes()),
            "command".to_owned(),
        ))
    } else {
        anyhow::bail!(
            "Transform '{}' has neither source nor command",
            node_def.transform
        );
    }
}

/// Resolve secrets hash for a transform.
///
/// Uses `ozzy_core::hash::secrets_hash()` to match the fetch.rs cache-hit fast path.
async fn resolve_secrets_hash(
    state: &AppState,
    project_id: Uuid,
    transform_def: &ozzy_core::toml_spec::TransformDef,
) -> Result<Option<String>, anyhow::Error> {
    if transform_def.secrets.is_empty() {
        return Ok(None);
    }
    let mut pairs: Vec<(String, String)> = Vec::new();
    for secret_name in &transform_def.secrets {
        let secret = state
            .db
            .get_secret(project_id, secret_name)
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Transform declares secret '{}' but it is not set for this project",
                    secret_name
                )
            })?;
        pairs.push((secret_name.clone(), secret.version_id.to_string()));
    }
    let refs: Vec<(&str, &str)> = pairs
        .iter()
        .map(|(n, v)| (n.as_str(), v.as_str()))
        .collect();
    Ok(ozzy_core::hash::secrets_hash(&refs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ozzy_core::toml_spec::{EdgeDef, EndpointDef, NodeDef};

    fn make_linear_endpoint() -> EndpointDef {
        let mut nodes = HashMap::new();
        nodes.insert(
            "step1".to_string(),
            NodeDef {
                transform: "qc".to_string(),
                params: HashMap::new(),
                machine: None,
            },
        );
        nodes.insert(
            "step2".to_string(),
            NodeDef {
                transform: "analyze".to_string(),
                params: HashMap::new(),
                machine: None,
            },
        );

        EndpointDef {
            description: Some("linear".to_string()),
            params: HashMap::new(),
            nodes,
            edges: vec![
                EdgeDef {
                    from: "data:raw".to_string(),
                    to: "step1.data".to_string(),
                },
                EdgeDef {
                    from: "step1".to_string(),
                    to: "step2.input".to_string(),
                },
            ],
        }
    }

    fn make_parallel_endpoint() -> EndpointDef {
        // step1 and step2 both depend only on data, step3 depends on both
        let mut nodes = HashMap::new();
        nodes.insert(
            "step1".to_string(),
            NodeDef {
                transform: "qc".to_string(),
                params: HashMap::new(),
                machine: None,
            },
        );
        nodes.insert(
            "step2".to_string(),
            NodeDef {
                transform: "filter".to_string(),
                params: HashMap::new(),
                machine: None,
            },
        );
        nodes.insert(
            "step3".to_string(),
            NodeDef {
                transform: "combine".to_string(),
                params: HashMap::new(),
                machine: None,
            },
        );

        EndpointDef {
            description: Some("parallel".to_string()),
            params: HashMap::new(),
            nodes,
            edges: vec![
                EdgeDef {
                    from: "data:raw".to_string(),
                    to: "step1.data".to_string(),
                },
                EdgeDef {
                    from: "data:raw".to_string(),
                    to: "step2.data".to_string(),
                },
                EdgeDef {
                    from: "step1".to_string(),
                    to: "step3.left".to_string(),
                },
                EdgeDef {
                    from: "step2".to_string(),
                    to: "step3.right".to_string(),
                },
            ],
        }
    }

    #[test]
    fn test_linear_waves() {
        let endpoint = make_linear_endpoint();
        let waves = compute_waves(&endpoint).unwrap();
        assert_eq!(waves.len(), 2);
        assert_eq!(waves[0], vec!["step1"]);
        assert_eq!(waves[1], vec!["step2"]);
    }

    #[test]
    fn test_parallel_waves() {
        let endpoint = make_parallel_endpoint();
        let waves = compute_waves(&endpoint).unwrap();
        assert_eq!(waves.len(), 2);
        // Wave 0: step1 and step2 (both depend only on data)
        let mut wave0 = waves[0].clone();
        wave0.sort();
        assert_eq!(wave0, vec!["step1", "step2"]);
        // Wave 1: step3
        assert_eq!(waves[1], vec!["step3"]);
    }

    #[test]
    fn test_single_node_wave() {
        let mut nodes = HashMap::new();
        nodes.insert(
            "only".to_string(),
            NodeDef {
                transform: "t".to_string(),
                params: HashMap::new(),
                machine: None,
            },
        );
        let endpoint = EndpointDef {
            description: None,
            params: HashMap::new(),
            nodes,
            edges: vec![EdgeDef {
                from: "data:x".to_string(),
                to: "only.input".to_string(),
            }],
        };
        let waves = compute_waves(&endpoint).unwrap();
        assert_eq!(waves.len(), 1);
        assert_eq!(waves[0], vec!["only"]);
    }

    #[test]
    fn test_diamond_dag_waves() {
        // A → B, A → C, B → D, C → D
        let mut nodes = HashMap::new();
        for name in &["a", "b", "c", "d"] {
            nodes.insert(
                name.to_string(),
                NodeDef {
                    transform: format!("t_{}", name),
                    params: HashMap::new(),
                    machine: None,
                },
            );
        }
        let endpoint = EndpointDef {
            description: None,
            params: HashMap::new(),
            nodes,
            edges: vec![
                EdgeDef {
                    from: "data:raw".to_string(),
                    to: "a.input".to_string(),
                },
                EdgeDef {
                    from: "a".to_string(),
                    to: "b.input".to_string(),
                },
                EdgeDef {
                    from: "a".to_string(),
                    to: "c.input".to_string(),
                },
                EdgeDef {
                    from: "b".to_string(),
                    to: "d.left".to_string(),
                },
                EdgeDef {
                    from: "c".to_string(),
                    to: "d.right".to_string(),
                },
            ],
        };
        let waves = compute_waves(&endpoint).unwrap();
        assert_eq!(waves.len(), 3);
        assert_eq!(waves[0], vec!["a"]);
        let mut wave1 = waves[1].clone();
        wave1.sort();
        assert_eq!(wave1, vec!["b", "c"]);
        assert_eq!(waves[2], vec!["d"]);
    }
}
