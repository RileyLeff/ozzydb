//! DAG orchestrator: runs jobs by executing nodes in parallel waves.
//!
//! Given a job, the orchestrator:
//! 1. Loads context from DB (published project revision, pinned snapshot, runtime defs)
//! 2. Computes topological waves (groups of nodes that can run in parallel)
//! 3. For each wave: check cache, dispatch uncached nodes in parallel
//! 4. Updates job/node status as execution progresses
//! 5. Stores output and sets job result on completion

use std::collections::{BTreeMap, HashMap, HashSet};

use base64::Engine as _;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::registry::{
    RegistrySnapshot, RuntimeTransformDef, load_published_project_revision_by_commit,
};
use crate::verification::{ensure_conformance_verified, verify_output_bytes};
use ozzy_types::syntax::{BuiltinConstructor, BuiltinType, TypeExpr};

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

    let published =
        load_published_project_revision_by_commit(&state.db, &state.registry_snapshots, commit.id)
            .await?;
    let invocation_actor_id = job.created_by.unwrap_or(commit.pushed_by);

    let endpoint_def = published
        .endpoints
        .get(&job.endpoint_name)
        .ok_or_else(|| anyhow::anyhow!("Endpoint '{}' not found", job.endpoint_name))?;
    let requested_inputs = decode_job_input_bindings(&job.input_bindings)?;
    let endpoint_inputs = super::super::api::v1::fetch::validate_and_resolve_endpoint_inputs(
        state,
        project.id,
        published.snapshot.as_ref(),
        endpoint_def,
        &requested_inputs,
    )
    .await?;

    let resolved_params: serde_json::Value = job.params.clone();
    // Safety: source_dir TempDir lives for the duration of run_job_inner. Spawned tasks
    // receive PathBuf clones, not TempDir refs. All tasks are awaited per-wave before
    // the function returns, so the TempDir outlives all tasks.
    let source_dir = if super::super::api::v1::fetch::endpoint_requires_source_code(
        endpoint_def,
        &published.runtime.transforms,
    )? {
        Some(super::super::api::v1::fetch::retrieve_source_code(state, &commit).await?)
    } else {
        None
    };
    let edge_map = super::super::api::v1::fetch::build_edge_map(endpoint_def);

    // Upload source code tarball to R2 for download by compute containers.
    // Done once per job (not per-node) since all nodes share the same commit's source.
    let mut source_cleanup_key: Option<String> = None;
    let source_download_url: Option<String> = if let Some(ref sd) = source_dir {
        let tar_bytes = create_source_tarball(sd.path())?;
        let key = format!("compute-source/{}.tar.gz", job_id);
        state.storage.store_by_key(&key, &tar_bytes).await?;
        let ttl = std::time::Duration::from_secs(state.config.compute.timeout_secs + 300);
        let url = state
            .storage
            .presigned_get_url_by_key_for_compute(&key, ttl)
            .await?;
        source_cleanup_key = Some(key);
        Some(url)
    } else {
        None
    };

    // Compute execution waves (groups of nodes that can run in parallel)
    let waves = match compute_waves(endpoint_def) {
        Ok(w) => w,
        Err(e) => {
            if let Some(ref key) = source_cleanup_key {
                let _ = state.storage.delete_by_key(key).await;
            }
            return Err(e);
        }
    };
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
            let project_id = project.id;
            let resolved_params = resolved_params.clone();
            let endpoint_inputs = endpoint_inputs.clone();
            let transforms = published.runtime.transforms.clone();
            let snapshot = published.snapshot.clone();
            let project_revision_id = published.row.id;
            let endpoint_def = endpoint_def.clone();
            let edge_map_for_node: Vec<(String, String)> = match edge_map.get(node_name.as_str()) {
                Some(edges) => edges
                    .iter()
                    .map(|(a, b)| (a.to_string(), b.to_string()))
                    .collect(),
                None => Vec::new(),
            };

            let source_dir_path = source_dir.as_ref().map(|d| d.path().to_path_buf());
            let node_outputs_snapshot: HashMap<String, NodeOutput> = node_outputs.clone();
            let source_download_url_clone = source_download_url.clone();

            handles.push(tokio::spawn(async move {
                let result = execute_node(
                    &state,
                    job_id,
                    &node_name,
                    &endpoint_def,
                    &transforms,
                    snapshot.as_ref(),
                    project_revision_id,
                    invocation_actor_id,
                    project_id,
                    &job_endpoint_name,
                    &resolved_params,
                    &endpoint_inputs,
                    &edge_map_for_node,
                    &node_outputs_snapshot,
                    source_dir_path.as_deref(),
                    source_download_url_clone.as_deref(),
                )
                .await;
                (node_name, result)
            }));
        }

        // Await all nodes in this wave. On first error, collect remaining
        // handles and drain them to prevent orphan tasks.
        let mut wave_error: Option<anyhow::Error> = None;
        let mut remaining_handles = Vec::new();
        for handle in handles {
            if wave_error.is_some() {
                remaining_handles.push(handle);
            } else {
                match handle.await {
                    Ok((node_name, Ok(output))) => {
                        node_outputs.insert(node_name, output);
                    }
                    Ok((node_name, Err(e))) => {
                        wave_error = Some(anyhow::anyhow!("Node '{}' failed: {}", node_name, e));
                    }
                    Err(e) => {
                        wave_error = Some(anyhow::anyhow!("Node execution task panicked: {}", e));
                    }
                }
            }
        }
        if let Some(e) = wave_error {
            // Await remaining handles to prevent orphan compute tasks
            for handle in remaining_handles {
                let _ = handle.await;
            }
            // Clean up source tarball before propagating error
            if let Some(ref key) = source_cleanup_key {
                let _ = state.storage.delete_by_key(key).await;
            }
            return Err(e);
        }
    }

    // Clean up Fly source tarball from R2 (best-effort, success path)
    if let Some(ref key) = source_cleanup_key {
        let _ = state.storage.delete_by_key(key).await;
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
    transforms: &HashMap<String, RuntimeTransformDef>,
    snapshot: &RegistrySnapshot,
    project_revision_id: Uuid,
    invocation_actor_id: Uuid,
    project_id: Uuid,
    endpoint_name: &str,
    resolved_params: &serde_json::Value,
    endpoint_inputs: &BTreeMap<String, super::super::api::v1::fetch::ResolvedEndpointInput>,
    edges_for_node: &[(String, String)],
    node_outputs: &HashMap<String, NodeOutput>,
    source_dir: Option<&std::path::Path>,
    source_download_url: Option<&str>,
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
    super::super::api::v1::fetch::validate_node_input_bindings(
        node_name,
        transform_def,
        edges_for_node.iter().map(|(name, _)| name.as_str()),
    )?;

    // Resolve inputs
    let mut input_artifact_ids: Vec<(String, String)> = Vec::new();
    for (input_name, source) in edges_for_node {
        let artifact_id = resolve_edge_source(source, endpoint_inputs, node_outputs).await?;
        input_artifact_ids.push((input_name.clone(), artifact_id));
    }

    let invocation_input_bindings =
        build_invocation_input_bindings(edges_for_node, endpoint_inputs, node_outputs)?;

    // Resolve params
    let node_params = super::super::api::v1::fetch::resolve_node_params(
        node_name,
        node_def,
        endpoint_def,
        resolved_params,
    );
    let params_hash = ozzy_core::hash::blake3_hash(serde_json::to_string(&node_params)?.as_bytes());

    // Resolve secrets hash
    let secrets_hash = resolve_secrets_hash(state, project_id, transform_def).await?;

    // Resolve environment
    let env_image = super::super::api::v1::fetch::resolve_environment_image_anyhow(
        state,
        &transform_def.environment,
    )
    .await?;

    // Compute source hash
    let source_hash = compute_source_hash(transform_def, node_def, source_dir)?;

    let input_refs: Vec<(&str, &str)> = input_artifact_ids
        .iter()
        .map(|(name, artifact_id)| (name.as_str(), artifact_id.as_str()))
        .collect();

    let mat_hash = ozzy_core::hash::materialized_hash(
        &input_refs,
        &transform_def.row_id.to_string(),
        &transform_def.environment.row_id.to_string(),
        &source_hash,
        &params_hash,
        secrets_hash.as_deref(),
    );

    // Check materialized cache
    if let Some(cached) = state.db.get_materialized_cache(&mat_hash).await? {
        let cached_artifact = state
            .db
            .get_v4_artifact(cached.output_artifact_id)
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Materialized cache '{}' references missing artifact '{}'",
                    mat_hash,
                    cached.output_artifact_id
                )
            })?;
        let (_, output_port) =
            super::super::api::v1::fetch::single_output_port(&node_def.transform, transform_def)?;
        let (_, output_type) = snapshot.resolve_type_ref(&output_port.ty)?;
        let conformance = state
            .db
            .get_v4_conformance_record(cached_artifact.id, output_type.id)
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Materialized cache '{}' references artifact '{}' without conformance for output type '{}'",
                    mat_hash,
                    cached_artifact.id,
                    output_port.ty.name
                )
            })?;
        let conformance = ensure_conformance_verified(
            state,
            snapshot,
            &cached_artifact,
            &conformance,
            &output_port.ty,
        )
        .await?;
        if conformance.status == "rejected" {
            anyhow::bail!(
                "Materialized cache '{}' references rejected artifact '{}' for output type '{}'",
                mat_hash,
                cached_artifact.id,
                output_port.ty.name
            );
        }
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
            artifact_id: cached.output_artifact_id,
        });
    }

    let invocation = state
        .db
        .insert_v4_invocation(
            project_revision_id,
            transform_def.row_id,
            Some(endpoint_name),
            Some(node_name),
            node_params.clone(),
            &params_hash,
            invocation_input_bindings.clone(),
            json!({}),
            "running",
            Some(invocation_actor_id),
        )
        .await?;

    // Resolve the server-selected compute backend. Provider realization is not
    // part of the authored endpoint model in v4.
    let backend = state.compute.backend()?;

    let env_image_ref = env_image
        .as_ref()
        .map(|img| img.image_ref.clone())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Environment '{}' has not been built yet",
                transform_def.environment.versioned_name
            )
        })?;

    if env_image.as_ref().and_then(|img| img.built_at).is_none() {
        anyhow::bail!(
            "Environment '{}' is still building",
            transform_def.environment.versioned_name
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
        let input_names: Vec<&str> = transform_def
            .inputs
            .ports
            .keys()
            .map(String::as_str)
            .collect();
        crate::runners::command::generate_shell_wrapper(command, &input_names)
    } else {
        anyhow::bail!(
            "Transform '{}' has neither source nor command",
            node_def.transform,
        );
    };

    let runner_type = if transform_def.source.is_some() {
        let source = transform_def
            .source
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("missing source for source-based transform"))?;
        crate::runners::detect_runner_type(source).ok_or_else(|| {
            anyhow::anyhow!(
                "Unsupported source file type in transform '{}'",
                node_def.transform
            )
        })?
    } else {
        crate::runners::RunnerType::Command
    };

    let init_script = crate::runners::init::generate_init(runner_type);

    // Build compute inputs (for manifest only)
    let (compute_inputs, downloads) = build_runtime_inputs(
        state,
        snapshot,
        project_id,
        edges_for_node,
        endpoint_inputs,
        node_outputs,
    )
    .await?;
    let input_manifest = crate::compute::build_input_manifest(&compute_inputs);
    let param_env_vars = crate::compute::build_param_env_vars(&node_params);

    let mut env_vars: HashMap<String, String> = HashMap::new();
    env_vars.insert(
        "OZZY_PARAMS".to_string(),
        serde_json::to_string(&node_params)?,
    );
    env_vars.insert(
        "OZZY_INPUT_MANIFEST".to_string(),
        serde_json::to_string(&input_manifest)?,
    );
    for (key, value) in param_env_vars {
        env_vars.insert(key, value);
    }

    // Build presigned download URLs for each input (all backends use presigned URLs)
    if !downloads.is_empty() {
        env_vars.insert(
            "OZZY_INPUT_DOWNLOADS".to_string(),
            serde_json::to_string(&downloads)?,
        );
    }

    // Add source code download URL (if source was uploaded by run_job_inner)
    if let Some(url) = source_download_url {
        env_vars.insert("OZZY_SOURCE_DOWNLOAD".to_string(), url.to_string());
    }

    // Inject secrets (always via R2 presigned URL for all backends)
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
    let mut secrets_cleanup_key: Option<String> = None;
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

        let mut decrypted_secrets: HashMap<String, String> = HashMap::new();
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
                .ok_or_else(|| anyhow::anyhow!("Required secret '{}' not found", secret_name))?;
            let decrypted =
                super::super::api::v1::fetch::decrypt_secret(&secret.encrypted_value, enc_key)?;
            decrypted_secrets.insert(secret_name.clone(), decrypted);
        }

        // Upload secrets to R2, pass presigned URL as env var
        let prepared = super::secrets::prepare_secrets(
            &state.storage,
            job_id,
            node_name,
            &decrypted_secrets,
            state.config.compute.timeout_secs,
        )
        .await?;
        env_vars.insert("OZZY_SECRETS_URL".to_string(), prepared.url);
        secrets_cleanup_key = Some(prepared.r2_key);
    }

    // Base64-encode init + runner scripts into env vars
    let init_b64 = base64::engine::general_purpose::STANDARD.encode(&init_script);
    env_vars.insert("OZZY_INIT_SCRIPT_B64".to_string(), init_b64);

    let runner_b64 = base64::engine::general_purpose::STANDARD.encode(&runner_script);
    env_vars.insert("OZZY_RUNNER_SCRIPT_B64".to_string(), runner_b64);

    // Determinism env vars
    env_vars.insert("PYTHONHASHSEED".to_string(), "0".to_string());
    env_vars.insert("OMP_NUM_THREADS".to_string(), "1".to_string());
    env_vars.insert("MKL_NUM_THREADS".to_string(), "1".to_string());
    env_vars.insert("OPENBLAS_NUM_THREADS".to_string(), "1".to_string());
    env_vars.insert("NUMEXPR_NUM_THREADS".to_string(), "1".to_string());
    env_vars.insert("VECLIB_MAXIMUM_THREADS".to_string(), "1".to_string());

    // Generate presigned PUT URL for output upload
    let output_temp_key = format!("compute-output/{}/{}.tar.gz", job_id, node_name);
    let put_ttl = std::time::Duration::from_secs(state.config.compute.timeout_secs + 300);
    let output_upload_url = state
        .storage
        .presigned_put_url_for_compute(&output_temp_key, put_ttl)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to generate output upload URL: {}", e))?;
    env_vars.insert("OZZY_OUTPUT_UPLOAD_URL".to_string(), output_upload_url);

    // Execute via compute backend
    let compute_request = crate::compute::ComputeRequest {
        image: env_image_ref,
        env_vars,
        timeout_secs: state.config.compute.timeout_secs,
        network_enabled: transform_def.network,
    };

    // Execute compute and download output from R2, ensuring cleanup on all paths
    let compute_result: Result<(Vec<u8>, String, u64), anyhow::Error> = async {
        let result = backend
            .run(&compute_request)
            .await
            .map_err(|e| anyhow::anyhow!("Compute execution failed: {}", e))?;

        if !result.success() {
            let _ = state.storage.delete_by_key(&output_temp_key).await;
            anyhow::bail!(
                "Transform '{}' failed (exit {}): {}",
                node_def.transform,
                result.exit_code,
                result.logs
            );
        }

        let compute_duration_ms = result.duration_ms;

        // Download output tarball from R2 and extract to temp dir
        let workspace = std::path::PathBuf::from(&state.config.compute.tmpdir)
            .join(format!("output-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&workspace).await?;
        let output_dir = workspace.join("output");
        tokio::fs::create_dir_all(&output_dir).await?;

        let download_result: Result<(Vec<u8>, String), anyhow::Error> = async {
            let tarball_bytes = state.storage.get_by_key(&output_temp_key).await?;

            let cursor = std::io::Cursor::new(&tarball_bytes);
            let gz = flate2::read::GzDecoder::new(cursor);
            let mut archive = tar::Archive::new(gz);
            archive.set_preserve_permissions(false);
            archive.set_unpack_xattrs(false);
            archive.set_overwrite(false);
            archive
                .unpack(&output_dir)
                .map_err(|e| anyhow::anyhow!("Failed to extract output tarball: {}", e))?;

            let output_files =
                super::super::api::v1::fetch::list_output_files_anyhow(&output_dir).await?;
            let primary_output = super::super::api::v1::fetch::find_primary_output(&output_files)
                .ok_or_else(|| {
                anyhow::anyhow!(
                    "Transform '{}' produced no output files",
                    node_def.transform
                )
            })?;
            let content_type =
                super::super::api::v1::fetch::infer_output_content_type(primary_output);
            let bytes = tokio::fs::read(primary_output).await?;
            Ok((bytes, content_type))
        }
        .await;

        // Clean up workspace and R2 temp key
        let _ = tokio::fs::remove_dir_all(&workspace).await;
        let _ = state.storage.delete_by_key(&output_temp_key).await;

        let (bytes, ct) = download_result?;
        Ok((bytes, ct, compute_duration_ms))
    }
    .await;

    // Always clean up secrets blob (best-effort), regardless of compute outcome
    if let Some(ref key) = secrets_cleanup_key {
        let _ = super::secrets::cleanup_secrets(&state.storage, key).await;
    }

    let (output_bytes, output_content_type, compute_duration_ms) = match compute_result {
        Ok(output) => output,
        Err(err) => {
            if let Err(mark_err) = state
                .db
                .mark_v4_invocation_failed(invocation.id, &err.to_string())
                .await
            {
                return Err(anyhow::anyhow!(
                    "node '{}' failed: {}; additionally failed to mark invocation {} failed: {}",
                    node_name,
                    err,
                    invocation.id,
                    mark_err
                ));
            }
            return Err(err);
        }
    };
    let output_hash = ozzy_core::hash::blake3_hash(&output_bytes);
    let output_byte_size = output_bytes.len() as i64;
    let output_ext = super::super::api::v1::fetch::content_type_to_extension(&output_content_type);

    // Store output (use store_with_hash to avoid redundant blake3 computation)
    state
        .storage
        .store_with_hash(&output_hash, &output_bytes, &output_ext)
        .await?;
    let output_r2_key = state.storage.storage_key(&output_hash, &output_ext)?;

    let (output_port_name, output_type_row_id, output_type_ref) =
        resolve_single_output_type(snapshot, transform_def)?;
    let (output_artifact, output_conformance, output_bindings) = match state
        .db
        .persist_v4_invocation_output(
            invocation.id,
            project_id,
            &output_port_name,
            &output_hash,
            output_type_row_id,
            &output_content_type,
            invocation_actor_id,
        )
        .await
    {
        Ok(result) => result,
        Err(err) => {
            if let Err(mark_err) = state
                .db
                .mark_v4_invocation_failed(invocation.id, &err.to_string())
                .await
            {
                return Err(anyhow::anyhow!(
                    "node '{}' output persistence failed: {}; additionally failed to mark invocation {} failed: {}",
                    node_name,
                    err,
                    invocation.id,
                    mark_err
                ));
            }
            return Err(err.into());
        }
    };

    let verification_report = match verify_output_bytes(
        snapshot,
        &output_type_ref,
        &output_content_type,
        &output_bytes,
    ) {
        Ok(report) => report,
        Err(err) => {
            let _ = state
                .db
                .record_v4_verification_failure(output_conformance.id, &err.as_failure())
                .await;
            if let Err(mark_err) = state
                .db
                .mark_v4_invocation_failed(invocation.id, &err.to_string())
                .await
            {
                return Err(anyhow::anyhow!(
                    "node '{}' output verification setup failed: {}; additionally failed to mark invocation {} failed: {}",
                    node_name,
                    err,
                    invocation.id,
                    mark_err
                ));
            }
            return Err(err.into());
        }
    };

    if let Err(err) = state
        .db
        .record_v4_verification_report(output_conformance.id, &verification_report)
        .await
    {
        if let Err(mark_err) = state
            .db
            .mark_v4_invocation_failed(invocation.id, &err.to_string())
            .await
        {
            return Err(anyhow::anyhow!(
                "node '{}' failed to record output verification report: {}; additionally failed to mark invocation {} failed: {}",
                node_name,
                err,
                invocation.id,
                mark_err
            ));
        }
        return Err(err.into());
    }

    if verification_report.verdict == ozzy_types::verify::VerificationVerdict::Rejected {
        let rejection = verification_report.diagnostics.join("; ");
        let message = format!(
            "Transform '{}' produced output rejected by '{}': {}",
            node_def.transform,
            output_type_ref.name,
            if rejection.is_empty() {
                "verification rejected"
            } else {
                &rejection
            }
        );
        if let Err(mark_err) = state
            .db
            .mark_v4_invocation_failed(invocation.id, &message)
            .await
        {
            return Err(anyhow::anyhow!(
                "node '{}' output verification rejected: {}; additionally failed to mark invocation {} failed: {}",
                node_name,
                message,
                invocation.id,
                mark_err
            ));
        }
        anyhow::bail!(message);
    }

    if let Err(err) = state
        .db
        .mark_v4_invocation_succeeded(invocation.id, output_bindings)
        .await
    {
        return Err(anyhow::anyhow!(
            "node '{}' completed output verification but failed to mark invocation {} succeeded: {}",
            node_name,
            invocation.id,
            err
        ));
    }

    // Insert materialized cache record
    state
        .db
        .insert_materialized_cache(
            &mat_hash,
            project_id,
            project_revision_id,
            endpoint_name,
            node_name,
            transform_def.row_id,
            transform_def.environment.row_id,
            &params_hash,
            &invocation_input_bindings,
            &source_hash,
            secrets_hash.as_deref(),
            output_artifact.id,
            &output_hash,
            &output_r2_key,
            &output_content_type,
            output_byte_size,
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
        artifact_id: output_artifact.id,
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
    pub artifact_id: Uuid,
}

fn decode_job_input_bindings(
    input_bindings: &serde_json::Value,
) -> Result<BTreeMap<String, Uuid>, anyhow::Error> {
    let object = input_bindings
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("Job input bindings must be a JSON object"))?;
    object
        .iter()
        .map(|(name, value)| {
            let id = value
                .as_str()
                .ok_or_else(|| {
                    anyhow::anyhow!("Job input binding '{}' must be a UUID string", name)
                })
                .and_then(|raw| {
                    Uuid::parse_str(raw).map_err(|e| {
                        anyhow::anyhow!(
                            "Job input binding '{}' has invalid UUID '{}': {}",
                            name,
                            raw,
                            e
                        )
                    })
                })?;
            Ok((name.clone(), id))
        })
        .collect()
}

async fn build_runtime_inputs(
    state: &AppState,
    snapshot: &RegistrySnapshot,
    project_id: Uuid,
    edges_for_node: &[(String, String)],
    endpoint_inputs: &BTreeMap<String, super::super::api::v1::fetch::ResolvedEndpointInput>,
    node_outputs: &HashMap<String, NodeOutput>,
) -> Result<
    (
        BTreeMap<String, crate::compute::InputSpec>,
        Vec<serde_json::Value>,
    ),
    anyhow::Error,
> {
    let mut manifest = BTreeMap::new();
    let mut downloads = Vec::new();

    for (input_name, source) in edges_for_node {
        let spec = if let Some(endpoint_input_name) = source.strip_prefix("input:") {
            let binding = endpoint_inputs.get(endpoint_input_name).ok_or_else(|| {
                anyhow::anyhow!(
                    "Endpoint input '{}' is not available for node input '{}'",
                    endpoint_input_name,
                    input_name
                )
            })?;
            let expected_expr = snapshot.expanded_type_expr(&binding.type_ref)?;
            materialize_artifact_input(
                state,
                project_id,
                &binding.artifact,
                &expected_expr,
                &format!("/workspace/inputs/{}", input_name),
                &mut downloads,
            )
            .await?
        } else {
            let output = node_outputs.get(source).ok_or_else(|| {
                anyhow::anyhow!(
                    "Node '{}' output is not available for input '{}'",
                    source,
                    input_name
                )
            })?;
            let loader = input_loader_from_content_type(&output.content_type);
            let ext = super::super::api::v1::fetch::content_type_to_extension(&output.content_type);
            let url = state
                .storage
                .presigned_get_url_for_compute(
                    &output.output_hash,
                    &ext,
                    std::time::Duration::from_secs(state.config.compute.timeout_secs + 300),
                )
                .await?;
            let path = format!("/workspace/inputs/{}", input_name);
            downloads.push(serde_json::json!({
                "name": input_name,
                "url": url,
                "path": path,
            }));
            crate::compute::InputSpec::Blob { path, loader }
        };

        manifest.insert(input_name.clone(), spec);
    }

    Ok((manifest, downloads))
}

#[async_recursion::async_recursion]
async fn materialize_artifact_input(
    state: &AppState,
    project_id: Uuid,
    artifact: &crate::db::v4::StoredArtifact,
    expected_expr: &TypeExpr,
    base_path: &str,
    downloads: &mut Vec<serde_json::Value>,
) -> Result<crate::compute::InputSpec, anyhow::Error> {
    match artifact.artifact_kind.as_str() {
        "blob" => {
            let content_hash = artifact.content_hash.as_ref().ok_or_else(|| {
                anyhow::anyhow!("Blob artifact '{}' is missing a content hash", artifact.id)
            })?;
            let loader = infer_blob_loader_from_expr(expected_expr)?;
            let ext = loader_extension(&loader);
            let url = state
                .storage
                .presigned_get_url_for_compute(
                    content_hash,
                    ext,
                    std::time::Duration::from_secs(state.config.compute.timeout_secs + 300),
                )
                .await?;
            downloads.push(serde_json::json!({
                "name": artifact.id.to_string(),
                "url": url,
                "path": base_path,
            }));
            Ok(crate::compute::InputSpec::Blob {
                path: base_path.to_string(),
                loader,
            })
        }
        "manifest" => {
            let manifest = state.db.decode_v4_artifact_manifest(artifact)?;
            match (manifest, expected_expr) {
                (
                    ozzy_core::artifacts::ArtifactManifest::Collection { items },
                    TypeExpr::Collection(item_ty),
                ) => {
                    let mut item_specs = Vec::new();
                    for (idx, entry) in items.iter().enumerate() {
                        let child = state
                            .db
                            .get_v4_artifact(entry.artifact_id)
                            .await?
                            .ok_or_else(|| {
                                anyhow::anyhow!(
                                    "Manifest artifact '{}' references missing artifact '{}'",
                                    artifact.id,
                                    entry.artifact_id
                                )
                            })?;
                        if child.project_id != project_id {
                            anyhow::bail!(
                                "Manifest artifact '{}' references artifact '{}' outside project '{}'",
                                artifact.id,
                                child.id,
                                project_id
                            );
                        }
                        let child_path = format!("{}/item_{:06}", base_path, idx);
                        item_specs.push(
                            materialize_artifact_input(
                                state,
                                project_id,
                                &child,
                                item_ty.as_ref(),
                                &child_path,
                                downloads,
                            )
                            .await?,
                        );
                    }
                    Ok(crate::compute::InputSpec::Collection { items: item_specs })
                }
                (
                    ozzy_core::artifacts::ArtifactManifest::Bundle { entries },
                    TypeExpr::Record(record),
                ) => {
                    let mut bundle_entries = BTreeMap::new();
                    let expected_fields = record
                        .fields
                        .iter()
                        .map(|field| (field.name.clone(), field))
                        .collect::<BTreeMap<_, _>>();

                    for field in &record.fields {
                        let entry = entries.get(&field.name).ok_or_else(|| {
                            anyhow::anyhow!(
                                "Bundle artifact '{}' is missing required entry '{}'",
                                artifact.id,
                                field.name
                            )
                        })?;
                        let child = state
                            .db
                            .get_v4_artifact(entry.artifact_id)
                            .await?
                            .ok_or_else(|| {
                                anyhow::anyhow!(
                                    "Bundle artifact '{}' references missing artifact '{}'",
                                    artifact.id,
                                    entry.artifact_id
                                )
                            })?;
                        if child.project_id != project_id {
                            anyhow::bail!(
                                "Bundle artifact '{}' references artifact '{}' outside project '{}'",
                                artifact.id,
                                child.id,
                                project_id
                            );
                        }
                        let child_path = format!("{}/{}", base_path, field.name);
                        let child_spec = materialize_artifact_input(
                            state,
                            project_id,
                            &child,
                            &field.ty,
                            &child_path,
                            downloads,
                        )
                        .await?;
                        bundle_entries.insert(field.name.clone(), child_spec);
                    }

                    if !record.open {
                        let unexpected = entries
                            .keys()
                            .filter(|name| !expected_fields.contains_key(*name))
                            .cloned()
                            .collect::<Vec<_>>();
                        if !unexpected.is_empty() {
                            anyhow::bail!(
                                "Bundle artifact '{}' has unexpected entries {:?} for a closed record type",
                                artifact.id,
                                unexpected
                            );
                        }
                    }

                    Ok(crate::compute::InputSpec::Bundle {
                        entries: bundle_entries,
                    })
                }
                (ozzy_core::artifacts::ArtifactManifest::Collection { .. }, other) => {
                    anyhow::bail!(
                        "Manifest artifact '{}' is a collection but expected type '{}' is not a collection",
                        artifact.id,
                        describe_type_expr(other)
                    )
                }
                (ozzy_core::artifacts::ArtifactManifest::Bundle { .. }, other) => {
                    anyhow::bail!(
                        "Manifest artifact '{}' is a bundle but expected type '{}' is not a record",
                        artifact.id,
                        describe_type_expr(other)
                    )
                }
            }
        }
        other => anyhow::bail!(
            "Artifact '{}' has unsupported runtime kind '{}'",
            artifact.id,
            other
        ),
    }
}

fn infer_blob_loader_from_expr(
    expr: &TypeExpr,
) -> Result<crate::compute::InputLoader, anyhow::Error> {
    if contains_builtin(expr, BuiltinType::Parquet) {
        return Ok(crate::compute::InputLoader::Parquet);
    }
    if contains_constructor(expr, BuiltinConstructor::Csv) {
        return Ok(crate::compute::InputLoader::Csv);
    }
    if contains_builtin(expr, BuiltinType::Json) {
        return Ok(crate::compute::InputLoader::Json);
    }
    if contains_builtin(expr, BuiltinType::Utf8) || contains_builtin(expr, BuiltinType::String) {
        return Ok(crate::compute::InputLoader::Text);
    }
    if contains_builtin(expr, BuiltinType::Bytes) {
        return Ok(crate::compute::InputLoader::Bytes);
    }
    match expr {
        TypeExpr::Table(_) => anyhow::bail!(
            "Table type '{}' does not declare an executable encoding; add parquet/json/csv to the type",
            describe_type_expr(expr)
        ),
        TypeExpr::Record(_) | TypeExpr::Collection(_) => anyhow::bail!(
            "Composite type '{}' cannot be loaded as a blob",
            describe_type_expr(expr)
        ),
        _ => anyhow::bail!(
            "Type '{}' does not declare an executable blob loader",
            describe_type_expr(expr)
        ),
    }
}

fn contains_builtin(expr: &TypeExpr, needle: BuiltinType) -> bool {
    match expr {
        TypeExpr::Builtin(builtin) => *builtin == needle,
        TypeExpr::Intersection(parts) => parts.iter().any(|part| contains_builtin(part, needle)),
        _ => false,
    }
}

fn contains_constructor(expr: &TypeExpr, needle: BuiltinConstructor) -> bool {
    match expr {
        TypeExpr::Constructor(constructor) => constructor.name == needle,
        TypeExpr::Intersection(parts) => {
            parts.iter().any(|part| contains_constructor(part, needle))
        }
        _ => false,
    }
}

fn loader_extension(loader: &crate::compute::InputLoader) -> &'static str {
    match loader {
        crate::compute::InputLoader::Bytes => "bin",
        crate::compute::InputLoader::Csv => "csv",
        crate::compute::InputLoader::Json => "json",
        crate::compute::InputLoader::Parquet => "parquet",
        crate::compute::InputLoader::Text => "txt",
    }
}

fn input_loader_from_content_type(content_type: &str) -> crate::compute::InputLoader {
    match content_type {
        "application/vnd.apache.parquet" => crate::compute::InputLoader::Parquet,
        "application/json" => crate::compute::InputLoader::Json,
        "text/csv" => crate::compute::InputLoader::Csv,
        "text/plain" => crate::compute::InputLoader::Text,
        _ => crate::compute::InputLoader::Bytes,
    }
}

fn describe_type_expr(expr: &TypeExpr) -> String {
    match expr {
        TypeExpr::Builtin(builtin) => builtin.as_str().to_string(),
        TypeExpr::Ref(type_ref) => match &type_ref.version {
            Some(version) => format!("{}@{}", type_ref.name, version),
            None => type_ref.name.clone(),
        },
        TypeExpr::Intersection(_) => "intersection".to_string(),
        TypeExpr::Constructor(constructor) => constructor.name.as_str().to_string(),
        TypeExpr::Record(_) => "record".to_string(),
        TypeExpr::Collection(_) => "collection".to_string(),
        TypeExpr::Table(_) => "table".to_string(),
        TypeExpr::Never => "never".to_string(),
    }
}

fn build_invocation_input_bindings(
    edges_for_node: &[(String, String)],
    endpoint_inputs: &BTreeMap<String, super::super::api::v1::fetch::ResolvedEndpointInput>,
    node_outputs: &HashMap<String, NodeOutput>,
) -> Result<serde_json::Value, anyhow::Error> {
    let mut bindings = serde_json::Map::new();
    for (input_name, source) in edges_for_node {
        let artifact_id = if let Some(endpoint_input_name) = source.strip_prefix("input:") {
            endpoint_inputs
                .get(endpoint_input_name)
                .map(|binding| binding.artifact.id)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Endpoint input '{}' is not available for node input '{}'",
                        endpoint_input_name,
                        input_name
                    )
                })?
        } else {
            node_outputs
                .get(source)
                .map(|output| output.artifact_id)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Node '{}' output is not available for node input '{}'",
                        source,
                        input_name
                    )
                })?
        };
        bindings.insert(
            input_name.clone(),
            json!({
                "source": source,
                "artifact_id": artifact_id,
            }),
        );
    }

    Ok(serde_json::Value::Object(bindings))
}

fn resolve_single_output_type(
    snapshot: &RegistrySnapshot,
    transform_def: &RuntimeTransformDef,
) -> Result<(String, Uuid, ozzy_types::syntax::TypeRefExpr), anyhow::Error> {
    let (output_name, output_port) = super::super::api::v1::fetch::single_output_port(
        &transform_def.versioned_name.to_string(),
        transform_def,
    )?;
    let (_, stored_type) = snapshot.resolve_type_ref(&output_port.ty)?;
    Ok((
        output_name.to_string(),
        stored_type.id,
        output_port.ty.clone(),
    ))
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
        // If the source is a node name (not input: or endpoint:)
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

/// Create a gzipped tarball of a directory's contents for Fly source delivery.
fn create_source_tarball(source_dir: &std::path::Path) -> Result<Vec<u8>, anyhow::Error> {
    use std::io::Write;

    let mut tar_builder = tar::Builder::new(Vec::new());
    tar_builder.follow_symlinks(false);
    tar_builder.append_dir_all(".", source_dir)?;
    let tar_bytes = tar_builder.into_inner()?;

    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(&tar_bytes)?;
    Ok(gz.finish()?)
}

/// Resolve an edge source to the bound input artifact ID.
async fn resolve_edge_source(
    source: &str,
    endpoint_inputs: &BTreeMap<String, super::super::api::v1::fetch::ResolvedEndpointInput>,
    node_outputs: &HashMap<String, NodeOutput>,
) -> Result<String, anyhow::Error> {
    if let Some(input_name) = source.strip_prefix("input:") {
        let binding = endpoint_inputs
            .get(input_name)
            .ok_or_else(|| anyhow::anyhow!("Endpoint input '{}' is not available", input_name))?;
        Ok(binding.artifact.id.to_string())
    } else if source.starts_with("endpoint:") {
        anyhow::bail!("Cross-project endpoint dependencies ('{source}') are not yet implemented")
    } else {
        let output = node_outputs.get(source).ok_or_else(|| {
            anyhow::anyhow!(
                "Node '{}' output not available (execution order issue?)",
                source
            )
        })?;
        Ok(output.artifact_id.to_string())
    }
}

/// Compute source hash for a transform (orchestrator version).
fn compute_source_hash(
    transform_def: &RuntimeTransformDef,
    node_def: &ozzy_core::toml_spec::NodeDef,
    source_dir: Option<&std::path::Path>,
) -> Result<String, anyhow::Error> {
    if let Some(source) = &transform_def.source {
        let (_file_path, _func) = crate::runners::parse_source_ref(source).ok_or_else(|| {
            anyhow::anyhow!(
                "Invalid source ref '{}' for transform '{}'",
                source,
                node_def.transform
            )
        })?;
        let file_path_str = source.split(':').next().unwrap_or(source);
        let sd = source_dir.ok_or_else(|| {
            anyhow::anyhow!(
                "Source transform '{}' requires extracted source code, but none was loaded",
                node_def.transform
            )
        })?;
        let source_root = sd
            .canonicalize()
            .map_err(|e| anyhow::anyhow!("Failed to canonicalize source dir: {}", e))?;
        let full_path = sd.join(file_path_str);
        let canonical = full_path.canonicalize().map_err(|e| {
            anyhow::anyhow!(
                "Failed to canonicalize source path '{}' for transform '{}': {}",
                file_path_str,
                node_def.transform,
                e
            )
        })?;
        if !canonical.starts_with(&source_root) {
            anyhow::bail!("Source path '{}' escapes source directory", file_path_str);
        }
        let bytes = std::fs::read(&canonical).map_err(|e| {
            anyhow::anyhow!(
                "Failed to read source file '{}' for transform '{}': {}",
                file_path_str,
                node_def.transform,
                e
            )
        })?;
        Ok(ozzy_core::hash::blake3_hash(&bytes))
    } else if let Some(command) = &transform_def.command {
        Ok(ozzy_core::hash::blake3_hash(command.as_bytes()))
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
    transform_def: &RuntimeTransformDef,
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
        let mut inputs = ozzy_types::ports::TypedPortSet::default();
        inputs.insert(
            "raw",
            ozzy_types::ports::TypedPort::new(ozzy_types::syntax::TypeRefExpr::new(
                "std/Input",
                Some("1".to_string()),
            )),
        );
        nodes.insert(
            "step1".to_string(),
            NodeDef {
                transform: "qc".to_string(),
                params: HashMap::new(),
            },
        );
        nodes.insert(
            "step2".to_string(),
            NodeDef {
                transform: "analyze".to_string(),
                params: HashMap::new(),
            },
        );

        EndpointDef {
            description: Some("linear".to_string()),
            params: HashMap::new(),
            inputs,
            nodes,
            edges: vec![
                EdgeDef {
                    from: "input:raw".to_string(),
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
        let mut inputs = ozzy_types::ports::TypedPortSet::default();
        inputs.insert(
            "raw",
            ozzy_types::ports::TypedPort::new(ozzy_types::syntax::TypeRefExpr::new(
                "std/Input",
                Some("1".to_string()),
            )),
        );
        nodes.insert(
            "step1".to_string(),
            NodeDef {
                transform: "qc".to_string(),
                params: HashMap::new(),
            },
        );
        nodes.insert(
            "step2".to_string(),
            NodeDef {
                transform: "filter".to_string(),
                params: HashMap::new(),
            },
        );
        nodes.insert(
            "step3".to_string(),
            NodeDef {
                transform: "combine".to_string(),
                params: HashMap::new(),
            },
        );

        EndpointDef {
            description: Some("parallel".to_string()),
            params: HashMap::new(),
            inputs,
            nodes,
            edges: vec![
                EdgeDef {
                    from: "input:raw".to_string(),
                    to: "step1.data".to_string(),
                },
                EdgeDef {
                    from: "input:raw".to_string(),
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
            },
        );
        let endpoint = EndpointDef {
            description: None,
            params: HashMap::new(),
            inputs: ozzy_types::ports::TypedPortSet::default(),
            nodes,
            edges: vec![EdgeDef {
                from: "input:x".to_string(),
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
                },
            );
        }
        let endpoint = EndpointDef {
            description: None,
            params: HashMap::new(),
            inputs: ozzy_types::ports::TypedPortSet::default(),
            nodes,
            edges: vec![
                EdgeDef {
                    from: "input:raw".to_string(),
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

    #[test]
    fn test_build_invocation_input_bindings_carries_artifact_bindings() {
        let upstream_artifact_id = Uuid::new_v4();
        let edges = vec![
            ("raw".to_string(), "input:raw".to_string()),
            ("cleaned".to_string(), "step1".to_string()),
        ];
        let endpoint_inputs = BTreeMap::from([(
            "raw".to_string(),
            crate::api::v1::fetch::ResolvedEndpointInput {
                artifact: crate::db::v4::StoredArtifact {
                    id: Uuid::new_v4(),
                    project_id: Uuid::new_v4(),
                    artifact_kind: "blob".to_string(),
                    content_hash: Some("hash_raw".to_string()),
                    manifest: None,
                    source_invocation_id: None,
                    created_by: Uuid::new_v4(),
                    created_at: chrono::Utc::now(),
                },
                type_ref: ozzy_types::syntax::TypeRefExpr::new("std/Input", Some("1".to_string())),
            },
        )]);
        let node_outputs = HashMap::from([(
            "step1".to_string(),
            NodeOutput {
                materialized_hash: "mat_hash".to_string(),
                output_hash: "hash_step1".to_string(),
                content_type: "application/json".to_string(),
                byte_size: 7,
                cache_hit: false,
                artifact_id: upstream_artifact_id,
            },
        )]);

        let bindings =
            build_invocation_input_bindings(&edges, &endpoint_inputs, &node_outputs).unwrap();

        assert_eq!(bindings["raw"]["source"], "input:raw");
        assert_eq!(
            bindings["raw"]["artifact_id"],
            serde_json::Value::String(endpoint_inputs["raw"].artifact.id.to_string())
        );
        assert_eq!(bindings["cleaned"]["source"], "step1");
        assert_eq!(
            bindings["cleaned"]["artifact_id"],
            serde_json::Value::String(upstream_artifact_id.to_string())
        );
    }
}
