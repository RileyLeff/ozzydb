//! Fetch endpoint — resolves and executes an endpoint's DAG.
//!
//! `GET /v1/fetch/{owner}/{project}/{endpoint}` is the main execution endpoint.
//! It resolves the DAG, checks the cache at each node, executes uncached
//! transforms via the compute backend, and streams the final output.

use std::collections::{HashMap, HashSet, VecDeque};

use axum::{
    Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Deserialize;

use super::auth::ApiError;
use crate::AppState;
use crate::auth::middleware::MaybeAuthUser;

/// Build the fetch router.
pub fn router() -> Router<AppState> {
    Router::new().route("/{owner}/{slug}/{endpoint}", get(fetch_endpoint))
}

#[derive(Debug, Deserialize)]
struct FetchQuery {
    #[serde(rename = "ref")]
    ref_name: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    format: Option<String>,
    #[serde(flatten)]
    params: HashMap<String, serde_json::Value>,
}

/// Fetch and execute an endpoint.
async fn fetch_endpoint(
    State(state): State<AppState>,
    Path((owner, slug, endpoint_name)): Path<(String, String, String)>,
    Query(query): Query<FetchQuery>,
    auth: MaybeAuthUser,
) -> Result<Response, ApiError> {
    // ── 1. Resolve project → ref → commit ───────────────────────
    let project =
        state.db.get_project(&owner, &slug).await?.ok_or_else(|| {
            ApiError::not_found(format!("Project '{}/{}' not found", owner, slug))
        })?;

    super::access::enforce_read_access(&state, &project, &owner, &slug, &auth).await?;

    let commit = if let Some(ref ref_name) = query.ref_name {
        let r = state
            .db
            .resolve_ref(project.id, ref_name)
            .await?
            .ok_or_else(|| ApiError::not_found(format!("Ref '{}' not found", ref_name)))?;
        state
            .db
            .get_commit_by_id(r.commit_id)
            .await?
            .ok_or_else(|| {
                ApiError::Internal(anyhow::anyhow!(
                    "Ref '{}' points to missing commit {}",
                    ref_name,
                    r.commit_id
                ))
            })?
    } else {
        let commits = state.db.list_commits(project.id, 1).await?;
        commits.into_iter().next().ok_or_else(|| {
            ApiError::not_found("No commits found. Push a commit first with `ozzy push`.")
        })?
    };

    let commit_state = state.db.get_commit_state(commit.id).await?.ok_or_else(|| {
        ApiError::Internal(anyhow::anyhow!(
            "Commit state missing for commit {}",
            commit.id
        ))
    })?;

    // ── 2. Parse endpoint from commit state ─────────────────────
    let endpoints: HashMap<String, ozzy_core::toml_spec::EndpointDef> =
        serde_json::from_value(commit_state.endpoints.clone())
            .map_err(|e| ApiError::Internal(e.into()))?;

    let endpoint_def = endpoints
        .get(&endpoint_name)
        .ok_or_else(|| ApiError::not_found(format!("Endpoint '{}' not found", endpoint_name)))?;

    // ── 3. Check yank status ────────────────────────────────────
    if state
        .db
        .is_endpoint_yanked(project.id, &endpoint_name, commit.id)
        .await?
    {
        return Err(ApiError::Gone(format!(
            "Endpoint '{}' has been yanked at this commit",
            endpoint_name
        )));
    }

    // ── 4. Load transforms and environments ─────────────────────
    let transforms: HashMap<String, ozzy_core::toml_spec::TransformDef> =
        serde_json::from_value(commit_state.transforms.clone())
            .map_err(|e| ApiError::Internal(e.into()))?;

    let environments: HashMap<String, ozzy_core::toml_spec::EnvironmentDef> =
        serde_json::from_value(commit_state.environments.clone())
            .map_err(|e| ApiError::Internal(e.into()))?;

    // ── 5. Validate consumer params ─────────────────────────────
    let resolved_params = validate_and_resolve_params(endpoint_def, &query.params)?;

    // ── 6. Build execution order (topological sort) ─────────────
    let exec_order = build_execution_order(endpoint_def)?;

    // ── 6.5. Retrieve and extract source code ────────────────────
    // Source tarballs are cached during push. We extract once and reuse for all nodes.
    let source_dir = retrieve_source_code(&state, &commit).await;

    // ── 7. Resolve edge sources and execute DAG ─────────────────
    // Track the output hash of each node as we execute
    let mut node_outputs: HashMap<String, NodeOutput> = HashMap::new();

    // Build edge map for quick lookup
    let edge_map = build_edge_map(endpoint_def);

    // Platform fingerprint (server platform) — computed once for all nodes
    let platform = ozzy_core::platform::PlatformFingerprint::detect();
    let platform_hash = platform.hash();

    for node_name in &exec_order {
        let node_def = endpoint_def.nodes.get(node_name).ok_or_else(|| {
            ApiError::Internal(anyhow::anyhow!(
                "Node '{}' missing from endpoint",
                node_name
            ))
        })?;

        let transform_def = transforms.get(&node_def.transform).ok_or_else(|| {
            ApiError::Internal(anyhow::anyhow!(
                "Transform '{}' not found for node '{}'",
                node_def.transform,
                node_name
            ))
        })?;

        // Resolve inputs for this node
        let mut input_hashes: Vec<(&str, String)> = Vec::new();
        let empty_vec = vec![];
        let edges_for_node = edge_map.get(node_name.as_str()).unwrap_or(&empty_vec);

        for (input_name, source) in edges_for_node {
            let hash = resolve_edge_source(source, &state, project.id, &node_outputs).await?;
            input_hashes.push((input_name, hash));
        }

        // Resolve node params (static params + endpoint param binds)
        let node_params = resolve_node_params(node_name, node_def, endpoint_def, &resolved_params);
        let params_hash = ozzy_core::hash::blake3_hash(
            serde_json::to_string(&node_params)
                .unwrap_or_default()
                .as_bytes(),
        );

        // Resolve secrets hash
        let secrets_hash = resolve_secrets_hash(&state, project.id, transform_def).await?;

        // Resolve environment image
        let env_def = environments
            .get(&transform_def.environment)
            .ok_or_else(|| {
                ApiError::Internal(anyhow::anyhow!(
                    "Environment '{}' not found for transform '{}'",
                    transform_def.environment,
                    node_def.transform,
                ))
            })?;

        let (env_image, env_hash, lockfile_hash) =
            resolve_environment_image(&state, env_def, &commit.git_repo, &commit.git_commit_sha)
                .await?;

        // Compute materialized hash
        let input_refs: Vec<(&str, &str)> = input_hashes
            .iter()
            .map(|(name, hash)| (*name, hash.as_str()))
            .collect();

        // Compute source_hash matching CLI behavior:
        // - source transforms: hash the actual file contents
        // - command transforms: hash the command string
        let (source_hash, function_name) = if let Some(source) = &transform_def.source {
            let (file_path, func) = crate::runners::parse_source_ref(source).ok_or_else(|| {
                ApiError::Internal(anyhow::anyhow!(
                    "Invalid source ref '{}' for transform '{}'",
                    source,
                    node_def.transform
                ))
            })?;
            let hash = if let Some(ref sd) = source_dir {
                let full_path = sd.path().join(file_path);
                // Verify path stays within source dir (defense-in-depth)
                let canonical = full_path.canonicalize().ok();
                let within_source = canonical
                    .as_ref()
                    .and_then(|c| sd.path().canonicalize().ok().map(|sd| c.starts_with(sd)))
                    .unwrap_or(false);
                if !within_source {
                    return Err(ApiError::BadRequest(format!(
                        "Source path '{}' escapes source directory",
                        file_path,
                    )));
                }
                tokio::fs::read(&full_path)
                    .await
                    .map(|bytes| ozzy_core::hash::blake3_hash(&bytes))
                    .unwrap_or_else(|_| {
                        // Fallback: hash source ref + commit SHA if file not extractable
                        ozzy_core::hash::blake3_hash(
                            format!("{}:{}", node_def.transform, commit.git_commit_sha).as_bytes(),
                        )
                    })
            } else {
                // No source tarball available — fallback to synthetic hash
                ozzy_core::hash::blake3_hash(
                    format!("{}:{}", node_def.transform, commit.git_commit_sha).as_bytes(),
                )
            };
            (hash, func)
        } else if let Some(command) = &transform_def.command {
            (ozzy_core::hash::blake3_hash(command.as_bytes()), "command")
        } else {
            return Err(ApiError::Internal(anyhow::anyhow!(
                "Transform '{}' has neither source nor command",
                node_def.transform
            )));
        };

        let params_schema_hash = {
            if transform_def.params.is_empty() {
                ozzy_core::hash::blake3_hash(b"")
            } else {
                let mut sorted_params: Vec<_> = transform_def.params.iter().collect();
                sorted_params.sort_by_key(|(name, _)| name.as_str());
                let schema_str: String = sorted_params
                    .iter()
                    .map(|(name, def)| format!("{}:{}", name, def.type_))
                    .collect::<Vec<_>>()
                    .join("\0");
                ozzy_core::hash::blake3_hash(schema_str.as_bytes())
            }
        };

        let transform_hash = ozzy_core::hash::transform_hash(
            &source_hash,
            function_name,
            &lockfile_hash,
            &env_hash,
            &params_schema_hash,
        );

        let mat_hash = ozzy_core::hash::materialized_hash(
            &input_refs,
            &transform_hash,
            &params_hash,
            &platform_hash,
            secrets_hash.as_deref(),
        );

        // ── Check materialized cache ────────────────────────────
        if let Some(cached) = state.db.get_materialized_cache(&mat_hash).await? {
            state.db.touch_materialized_cache(&mat_hash).await?;

            tracing::info!(
                "Cache hit for node '{}': {}",
                node_name,
                mat_hash.get(..12).unwrap_or(&mat_hash)
            );

            node_outputs.insert(
                node_name.clone(),
                NodeOutput {
                    materialized_hash: mat_hash,
                    output_hash: cached.output_hash,
                    content_type: cached.output_content_type,
                    byte_size: cached.output_byte_size,
                    cache_hit: true,
                },
            );
            continue;
        }

        // ── Execute uncached node ───────────────────────────────
        if !state.config.compute.enabled {
            return Err(ApiError::service_unavailable(
                "Compute is not enabled on this server. Set COMPUTE_ENABLED=true.",
            ));
        }

        let env_image_ref = env_image
            .as_ref()
            .map(|img| img.image_ref.clone())
            .ok_or_else(|| {
                ApiError::service_unavailable(format!(
                    "Environment '{}' has not been built yet. Push again to trigger a build.",
                    transform_def.environment
                ))
            })?;

        // Check that the environment was actually built
        if env_image.as_ref().and_then(|img| img.built_at).is_none() {
            return Err(ApiError::service_unavailable(format!(
                "Environment '{}' is still building. Try again shortly.",
                transform_def.environment
            )));
        }

        // Generate runner script
        let runner_script = if let Some(source) = &transform_def.source {
            let (file_path, func_name) =
                crate::runners::validate_source_ref(source).map_err(|e| {
                    ApiError::BadRequest(format!(
                        "Invalid source reference '{}' in transform '{}': {}",
                        source, node_def.transform, e
                    ))
                })?;
            let runner_type = crate::runners::detect_runner_type(source).ok_or_else(|| {
                ApiError::BadRequest(format!(
                    "Unsupported source file type in '{}'. Only .py and .R files are supported.",
                    source
                ))
            })?;
            match runner_type {
                crate::runners::RunnerType::Python => {
                    crate::runners::python::generate(file_path, func_name)
                }
                crate::runners::RunnerType::R => crate::runners::r::generate(file_path, func_name),
                crate::runners::RunnerType::Command => {
                    return Err(ApiError::Internal(anyhow::anyhow!(
                        "Source-based transform incorrectly detected as Command type"
                    )));
                }
            }
        } else if let Some(command) = &transform_def.command {
            let input_names: Vec<&str> = transform_def.inputs.keys().map(|s| s.as_str()).collect();
            crate::runners::command::generate_shell_wrapper(command, &input_names)
        } else {
            return Err(ApiError::Internal(anyhow::anyhow!(
                "Transform '{}' has neither source nor command",
                node_def.transform,
            )));
        };

        let runner_ext = if transform_def.source.is_some() {
            let rt =
                crate::runners::detect_runner_type(transform_def.source.as_deref().unwrap_or(""))
                    .unwrap_or(crate::runners::RunnerType::Python); // already validated above
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
                .unwrap_or(crate::runners::RunnerType::Python) // already validated above
        } else {
            crate::runners::RunnerType::Command
        };

        let init_script = crate::runners::init::generate_docker_init(runner_type);

        // Build input manifest and env vars
        let compute_inputs: Vec<crate::compute::InputSpec> = Vec::new(); // TODO: resolve to local paths
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

        // Inject secrets (block reserved env var names to prevent overwriting runtime controls)
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
            let enc_key = state.config.secrets_encryption_key.as_ref().ok_or_else(|| {
                ApiError::service_unavailable(format!(
                    "Transform '{}' requires secrets but the server has no secrets encryption key configured",
                    node_def.transform
                ))
            })?;
            for secret_name in &transform_def.secrets {
                if secret_name.starts_with("OZZY_") {
                    return Err(ApiError::BadRequest(format!(
                        "Secret '{}' uses reserved prefix 'OZZY_'. Secrets must not override runtime environment variables.",
                        secret_name
                    )));
                }
                if RESERVED_SECRET_NAMES
                    .iter()
                    .any(|&r| r.eq_ignore_ascii_case(secret_name))
                {
                    return Err(ApiError::BadRequest(format!(
                        "Secret '{}' would override a reserved runtime environment variable. \
                         Choose a different name.",
                        secret_name
                    )));
                }
                if let Some(secret) = state.db.get_secret(project.id, secret_name).await? {
                    let decrypted =
                        decrypt_secret(&secret.encrypted_value, enc_key).map_err(|e| {
                            ApiError::Internal(anyhow::anyhow!(
                                "Failed to decrypt secret '{}': {}",
                                secret_name,
                                e
                            ))
                        })?;
                    env_vars.insert(secret_name.clone(), decrypted);
                }
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
            source_dir: source_dir.as_ref().map(|d| d.path().to_path_buf()),
        };

        let result = crate::compute::docker::run(&compute_request, &state.config.compute.tmpdir)
            .await
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("Compute execution failed: {}", e)))?;

        if !result.success() {
            let logs = result.logs.clone();
            let exit_code = result.exit_code;
            result.cleanup().await;
            return Err(ApiError::Internal(anyhow::anyhow!(
                "Transform '{}' failed (exit {}): {}",
                node_def.transform,
                exit_code,
                logs
            )));
        }

        // Read output from workspace, ensuring cleanup on any error path.
        let compute_duration_ms = result.duration_ms;
        let read_result: Result<(Vec<u8>, String), ApiError> = async {
            let output_files = list_output_files(&result.output_dir).await?;
            let primary_output = find_primary_output(&output_files).ok_or_else(|| {
                ApiError::Internal(anyhow::anyhow!(
                    "Transform '{}' produced no output files",
                    node_def.transform
                ))
            })?;
            let content_type = infer_output_content_type(primary_output);
            let bytes = tokio::fs::read(primary_output)
                .await
                .map_err(|e| ApiError::Internal(anyhow::anyhow!("Failed to read output: {}", e)))?;
            Ok((bytes, content_type))
        }
        .await;
        result.cleanup().await;
        let (output_bytes, output_content_type) = read_result?;
        let output_hash = ozzy_core::hash::blake3_hash(&output_bytes);
        let output_byte_size = output_bytes.len() as i64;
        let output_ext = content_type_to_extension(&output_content_type);

        // Store output in content storage (content-addressed by output_hash)
        state
            .storage
            .store(&output_bytes, &output_ext)
            .await
            .map_err(ApiError::Internal)?;
        let output_r2_key = state
            .storage
            .storage_key(&output_hash, &output_ext)
            .map_err(ApiError::Internal)?;

        // Insert materialized cache record
        let platform_str = serde_json::to_string(&platform).unwrap_or_default();
        state
            .db
            .insert_materialized_cache(
                &mat_hash,
                project.id,
                commit.id,
                &endpoint_name,
                node_name,
                &node_def.transform,
                &output_hash,
                &output_r2_key,
                &output_content_type,
                output_byte_size,
                &platform_str,
                1, // verification_tier: server-verified
            )
            .await
            .map_err(ApiError::Internal)?;

        tracing::info!(
            "Computed node '{}' ({}ms): {}",
            node_name,
            compute_duration_ms,
            mat_hash.get(..12).unwrap_or(&mat_hash)
        );

        node_outputs.insert(
            node_name.clone(),
            NodeOutput {
                materialized_hash: mat_hash,
                output_hash,
                content_type: output_content_type,
                byte_size: output_byte_size,
                cache_hit: false,
            },
        );
    }

    // ── 8. Return final node output ─────────────────────────────
    let final_node = find_terminal_node(endpoint_def)?;

    let final_output = node_outputs.get(final_node).ok_or_else(|| {
        ApiError::Internal(anyhow::anyhow!("Final node '{}' has no output", final_node))
    })?;

    // Fetch the output bytes from storage
    let final_ext = content_type_to_extension(&final_output.content_type);
    let output_bytes = state
        .storage
        .get(&final_output.output_hash, &final_ext)
        .await
        .map_err(ApiError::Internal)?;

    let any_miss = node_outputs.values().any(|o| !o.cache_hit);

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        final_output
            .content_type
            .parse()
            .unwrap_or(header::HeaderValue::from_static("application/octet-stream")),
    );
    headers.insert(
        "X-OzzyDB-Hash",
        final_output
            .materialized_hash
            .parse()
            .unwrap_or(header::HeaderValue::from_static("")),
    );
    headers.insert(
        "X-OzzyDB-Verification",
        header::HeaderValue::from_static("server-verified"),
    );
    headers.insert(
        "X-OzzyDB-Cache",
        if any_miss {
            header::HeaderValue::from_static("miss")
        } else {
            header::HeaderValue::from_static("hit")
        },
    );

    Ok((StatusCode::OK, headers, output_bytes.to_vec()).into_response())
}

// ── Internal types ────────────────────────────────────────────────

struct NodeOutput {
    materialized_hash: String,
    output_hash: String,
    content_type: String,
    #[allow(dead_code)]
    byte_size: i64,
    cache_hit: bool,
}

// ── Helpers ───────────────────────────────────────────────────────

/// Validate consumer params against endpoint param definitions.
fn validate_and_resolve_params(
    endpoint: &ozzy_core::toml_spec::EndpointDef,
    consumer_params: &HashMap<String, serde_json::Value>,
) -> Result<serde_json::Value, ApiError> {
    // Check for unrecognized params
    for key in consumer_params.keys() {
        if !endpoint.params.contains_key(key) {
            let available: Vec<&str> = endpoint.params.keys().map(|s| s.as_str()).collect();
            return Err(ApiError::BadRequest(format!(
                "Unknown parameter '{}'. Available: {:?}",
                key, available
            )));
        }
    }

    let mut resolved = serde_json::Map::new();

    for (name, param_def) in &endpoint.params {
        let value = if let Some(v) = consumer_params.get(name) {
            // Coerce string values from query params to declared type
            let coerced = coerce_param_value(v, &param_def.type_);
            // Validate type
            validate_param_value(name, &coerced, param_def)?;
            coerced
        } else if let Some(default) = &param_def.default {
            // Validate default value too (catches bad ozzy.toml definitions)
            validate_param_value(name, default, param_def)?;
            default.clone()
        } else {
            return Err(ApiError::BadRequest(format!(
                "Required parameter '{}' not provided",
                name
            )));
        };

        resolved.insert(name.clone(), value);
    }

    Ok(serde_json::Value::Object(resolved))
}

/// Coerce a query-string parameter value to the declared type.
///
/// URL query params are always strings. This function converts string values
/// to the appropriate JSON type (number, bool) based on the declared param type.
fn coerce_param_value(value: &serde_json::Value, declared_type: &str) -> serde_json::Value {
    if let serde_json::Value::String(s) = value {
        match declared_type {
            "float" | "number" => {
                if let Ok(n) = s.parse::<f64>() {
                    return serde_json::Value::from(n);
                }
            }
            "int" | "integer" => {
                if let Ok(n) = s.parse::<i64>() {
                    return serde_json::Value::from(n);
                }
            }
            "bool" | "boolean" => match s.as_str() {
                "true" | "1" | "yes" => return serde_json::Value::Bool(true),
                "false" | "0" | "no" => return serde_json::Value::Bool(false),
                _ => {}
            },
            _ => {} // "string" or unknown — keep as-is
        }
    }
    value.clone()
}

/// Validate a parameter value against its definition (min/max/enum).
fn validate_param_value(
    name: &str,
    value: &serde_json::Value,
    param_def: &ozzy_core::toml_spec::EndpointParamDef,
) -> Result<(), ApiError> {
    // Verify JSON type matches declared param type after coercion
    match param_def.type_.as_str() {
        "float" | "number" => {
            if !value.is_f64() && !value.is_i64() && !value.is_u64() {
                return Err(ApiError::BadRequest(format!(
                    "Parameter '{}': expected numeric value, got {:?}",
                    name, value
                )));
            }
        }
        "int" | "integer" => {
            if !value.is_i64() && !value.is_u64() {
                return Err(ApiError::BadRequest(format!(
                    "Parameter '{}': expected integer value, got {:?}",
                    name, value
                )));
            }
        }
        "bool" | "boolean" => {
            if !value.is_boolean() {
                return Err(ApiError::BadRequest(format!(
                    "Parameter '{}': expected boolean value, got {:?}",
                    name, value
                )));
            }
        }
        _ => {} // "string" or unknown — any JSON type is acceptable
    }

    // Check min/max for numeric types
    if let Some(num) = value.as_f64() {
        if let Some(min) = param_def.min {
            if num < min {
                return Err(ApiError::BadRequest(format!(
                    "Parameter '{}' value {} is below minimum {}",
                    name, num, min
                )));
            }
        }
        if let Some(max) = param_def.max {
            if num > max {
                return Err(ApiError::BadRequest(format!(
                    "Parameter '{}' value {} exceeds maximum {}",
                    name, num, max
                )));
            }
        }
    }

    // Check enum constraint
    if let Some(ref enum_values) = param_def.enum_values {
        if !enum_values.contains(value) {
            return Err(ApiError::BadRequest(format!(
                "Parameter '{}' value {:?} not in allowed values: {:?}",
                name, value, enum_values
            )));
        }
    }

    Ok(())
}

/// Build execution order via topological sort (Kahn's algorithm).
fn build_execution_order(
    endpoint: &ozzy_core::toml_spec::EndpointDef,
) -> Result<Vec<String>, ApiError> {
    let nodes: HashSet<&str> = endpoint.nodes.keys().map(|s| s.as_str()).collect();

    // Build adjacency: for each edge "nodeA → nodeB.input", nodeA must come before nodeB
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();

    for node in &nodes {
        in_degree.insert(node, 0);
        adj.insert(node, Vec::new());
    }

    for edge in &endpoint.edges {
        let to_node = edge.to.split('.').next().unwrap_or(&edge.to);

        // Only count edges from other nodes (not from data:/collection: sources)
        if nodes.contains(edge.from.as_str()) {
            adj.get_mut(edge.from.as_str()).map(|v| v.push(to_node));
            *in_degree.entry(to_node).or_insert(0) += 1;
        }
    }

    // Collect initial zero-degree nodes and sort for deterministic order
    let mut initial: Vec<&str> = in_degree
        .iter()
        .filter(|&(_, &deg)| deg == 0)
        .map(|(&node, _)| node)
        .collect();
    initial.sort();
    let mut queue: VecDeque<&str> = VecDeque::from(initial);

    let mut order: Vec<String> = Vec::new();
    while let Some(node) = queue.pop_front() {
        order.push(node.to_string());
        if let Some(neighbors) = adj.get(node) {
            let mut newly_ready: Vec<&str> = Vec::new();
            for &next in neighbors {
                if let Some(deg) = in_degree.get_mut(next) {
                    *deg -= 1;
                    if *deg == 0 {
                        newly_ready.push(next);
                    }
                }
            }
            newly_ready.sort();
            queue.extend(newly_ready);
        }
    }

    if order.len() != nodes.len() {
        return Err(ApiError::Internal(anyhow::anyhow!(
            "Cycle detected in endpoint DAG"
        )));
    }

    Ok(order)
}

/// Find the single terminal (sink) node of an endpoint DAG.
///
/// A terminal node has no outgoing edges to other nodes. If there are zero
/// or multiple terminal nodes, return an error.
fn find_terminal_node(endpoint: &ozzy_core::toml_spec::EndpointDef) -> Result<&str, ApiError> {
    let node_names: HashSet<&str> = endpoint.nodes.keys().map(|s| s.as_str()).collect();
    let mut has_outgoing: HashSet<&str> = HashSet::new();

    for edge in &endpoint.edges {
        // An edge FROM a node TO another node means the source node has outgoing edges
        if node_names.contains(edge.from.as_str()) {
            let to_node = edge.to.split('.').next().unwrap_or(&edge.to);
            if node_names.contains(to_node) && edge.from.as_str() != to_node {
                has_outgoing.insert(edge.from.as_str());
            }
        }
    }

    let terminals: Vec<&str> = node_names
        .iter()
        .filter(|n| !has_outgoing.contains(**n))
        .copied()
        .collect();

    match terminals.len() {
        0 => Err(ApiError::Internal(anyhow::anyhow!(
            "Endpoint has no terminal node (all nodes have outgoing edges)"
        ))),
        1 => Ok(terminals[0]),
        _ => {
            let mut names: Vec<&str> = terminals;
            names.sort();
            // For determinism, pick the lexicographically first terminal node
            // and log a warning about the ambiguity
            tracing::warn!(
                "Endpoint has multiple terminal nodes: {:?}. Using '{}'.",
                names,
                names[0]
            );
            Ok(names[0])
        }
    }
}

/// Build a map from node_name → [(input_name, edge_source)] for quick lookup.
fn build_edge_map<'a>(
    endpoint: &'a ozzy_core::toml_spec::EndpointDef,
) -> HashMap<&'a str, Vec<(&'a str, &'a str)>> {
    let mut map: HashMap<&str, Vec<(&str, &str)>> = HashMap::new();
    for edge in &endpoint.edges {
        // edge.to = "node_name.input_name"
        if let Some((node_name, input_name)) = edge.to.split_once('.') {
            map.entry(node_name)
                .or_default()
                .push((input_name, edge.from.as_str()));
        }
    }
    map
}

/// Resolve an edge source to a content hash.
async fn resolve_edge_source(
    source: &str,
    state: &AppState,
    project_id: uuid::Uuid,
    node_outputs: &HashMap<String, NodeOutput>,
) -> Result<String, ApiError> {
    if let Some(data_name) = source.strip_prefix("data:") {
        // Resolve data atom hash
        let atom = state
            .db
            .get_data_atom(project_id, data_name)
            .await?
            .ok_or_else(|| ApiError::not_found(format!("Data atom '{}' not found", data_name)))?;
        if atom.yanked {
            return Err(ApiError::Gone(format!(
                "Data atom '{}' has been yanked",
                data_name
            )));
        }
        Ok(atom.hash)
    } else if let Some(coll_name) = source.strip_prefix("collection:") {
        // Resolve collection version hash
        let collection = state
            .db
            .get_collection(project_id, coll_name)
            .await?
            .ok_or_else(|| ApiError::not_found(format!("Collection '{}' not found", coll_name)))?;
        if collection.yanked {
            return Err(ApiError::Gone(format!(
                "Collection '{}' has been yanked",
                coll_name
            )));
        }
        let version = state
            .db
            .get_latest_collection_version(collection.id)
            .await?
            .ok_or_else(|| {
                ApiError::not_found(format!("Collection '{}' has no versions", coll_name))
            })?;
        Ok(version.hash)
    } else if source.starts_with("endpoint:") {
        // Cross-project or same-project endpoint reference
        // TODO: implement recursive endpoint resolution
        Ok(ozzy_core::hash::blake3_hash(source.as_bytes()))
    } else {
        // Node reference within the same endpoint
        let output = node_outputs.get(source).ok_or_else(|| {
            ApiError::Internal(anyhow::anyhow!(
                "Node '{}' output not available (execution order issue?)",
                source
            ))
        })?;
        Ok(output.output_hash.clone())
    }
}

/// Retrieve and extract the source tarball for a commit.
///
/// Returns a temp directory containing the extracted source files, or `None`
/// if the source tarball is not available (e.g., not yet cached during push).
async fn retrieve_source_code(
    state: &AppState,
    commit: &crate::db::Commit,
) -> Option<tempfile::TempDir> {
    let source_storage =
        match crate::storage::ContentStorage::from_config_with_prefix(&state.config, "source") {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("Failed to create source storage: {}", e);
                return None;
            }
        };

    let tarball_bytes = match source_storage.get(&commit.git_commit_sha, "tar.gz").await {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::warn!(
                "Source tarball not found for commit {} (sha {}): {}",
                commit.id,
                commit.git_commit_sha,
                e
            );
            return None;
        }
    };

    let tmp = match tempfile::tempdir() {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("Failed to create temp dir for source extraction: {}", e);
            return None;
        }
    };

    // Write tarball to temp file, extract with strip-components=1 (GitHub prefix)
    let tarball_path = tmp.path().join("source.tar.gz");
    if let Err(e) = tokio::fs::write(&tarball_path, &tarball_bytes).await {
        tracing::warn!("Failed to write source tarball: {}", e);
        return None;
    }

    let output = match tokio::process::Command::new("tar")
        .arg("xzf")
        .arg(&tarball_path)
        .arg("-C")
        .arg(tmp.path())
        .arg("--strip-components=1")
        .output()
        .await
    {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!("Failed to run tar for source extraction: {}", e);
            return None;
        }
    };

    if !output.status.success() {
        tracing::warn!(
            "tar extraction failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        return None;
    }

    // Clean up the tarball file, keep the extracted contents
    tokio::fs::remove_file(&tarball_path).await.ok();

    Some(tmp)
}

/// Resolve the environment image for a transform.
///
/// For Prebuilt: looks up by image ref directly.
/// For BaseLockfile/Dockerfile: fetches content from git, computes env_hash,
/// looks up the built image in the DB.
/// Returns (env_image, env_hash, lockfile_hash).
/// lockfile_hash is blake3(lockfile_content) for BaseLockfile, blake3(b"") otherwise.
async fn resolve_environment_image(
    state: &AppState,
    env_def: &ozzy_core::toml_spec::EnvironmentDef,
    git_repo: &str,
    git_commit_sha: &str,
) -> Result<(Option<crate::db::EnvironmentImage>, String, String), ApiError> {
    let tier = env_def.tier().ok_or_else(|| {
        ApiError::Internal(anyhow::anyhow!(
            "Environment has invalid tier configuration"
        ))
    })?;

    match &tier {
        ozzy_core::toml_spec::EnvironmentTier::Prebuilt { image } => {
            let env_hash = ozzy_core::hash::blake3_hash(image.as_bytes());
            // For prebuilt, the image ref is already known — synthesize a ready record
            // rather than requiring an async DB insert to complete first.
            let env_image = crate::db::EnvironmentImage {
                id: uuid::Uuid::nil(),
                env_hash: env_hash.clone(),
                build_type: "prebuilt".to_string(),
                image_ref: image.clone(),
                base_image: None,
                build_log_r2_key: None,
                build_duration_ms: None,
                built_at: Some(chrono::Utc::now()),
                created_at: chrono::Utc::now(),
            };
            let lockfile_hash = ozzy_core::hash::blake3_hash(b"");
            Ok((Some(env_image), env_hash, lockfile_hash))
        }
        ozzy_core::toml_spec::EnvironmentTier::BaseLockfile { lockfile, .. } => {
            // Fetch lockfile content from git to compute env_hash
            let lockfile_bytes = state
                .git
                .get_file(git_repo, git_commit_sha, lockfile)
                .await
                .map_err(|e| {
                    ApiError::Internal(anyhow::anyhow!(
                        "Failed to fetch lockfile '{}': {}",
                        lockfile,
                        e
                    ))
                })?;
            let lockfile_hash = ozzy_core::hash::blake3_hash(&lockfile_bytes);
            let content = crate::environments::hash::EnvironmentContent {
                lockfile_content: Some(String::from_utf8_lossy(&lockfile_bytes).to_string()),
                ..Default::default()
            };
            let env_hash = crate::environments::hash::compute_env_hash(&tier, &content);
            let env_image = state.db.get_environment_image(&env_hash).await?;
            Ok((env_image, env_hash, lockfile_hash))
        }
        ozzy_core::toml_spec::EnvironmentTier::Dockerfile { dockerfile } => {
            // Fetch Dockerfile content from git to compute env_hash
            let dockerfile_bytes = state
                .git
                .get_file(git_repo, git_commit_sha, dockerfile)
                .await
                .map_err(|e| {
                    ApiError::Internal(anyhow::anyhow!(
                        "Failed to fetch Dockerfile '{}': {}",
                        dockerfile,
                        e
                    ))
                })?;
            let content = crate::environments::hash::EnvironmentContent {
                dockerfile_content: Some(String::from_utf8_lossy(&dockerfile_bytes).to_string()),
                ..Default::default()
            };
            let env_hash = crate::environments::hash::compute_env_hash(&tier, &content);
            let env_image = state.db.get_environment_image(&env_hash).await?;
            let lockfile_hash = ozzy_core::hash::blake3_hash(b"");
            Ok((env_image, env_hash, lockfile_hash))
        }
    }
}

/// Resolve params for a specific node: merge static node params with endpoint param binds.
fn resolve_node_params(
    node_name: &str,
    node_def: &ozzy_core::toml_spec::NodeDef,
    endpoint: &ozzy_core::toml_spec::EndpointDef,
    endpoint_params: &serde_json::Value,
) -> serde_json::Value {
    let mut params = serde_json::Map::new();

    // Start with node's static params
    for (key, value) in &node_def.params {
        params.insert(key.clone(), value.clone());
    }

    // Override with endpoint param binds (binds format: "node_name.param_name")
    let ep_params_obj = endpoint_params.as_object();
    for (ep_param_name, ep_param_def) in &endpoint.params {
        if let Some((bind_node, bind_param)) = ep_param_def.binds.split_once('.') {
            if bind_node == node_name {
                if let Some(obj) = &ep_params_obj {
                    if let Some(value) = obj.get(ep_param_name) {
                        params.insert(bind_param.to_string(), value.clone());
                    }
                }
            }
        }
    }

    serde_json::Value::Object(params)
}

/// Resolve secrets hash for a transform.
async fn resolve_secrets_hash(
    state: &AppState,
    project_id: uuid::Uuid,
    transform: &ozzy_core::toml_spec::TransformDef,
) -> Result<Option<String>, ApiError> {
    if transform.secrets.is_empty() {
        return Ok(None);
    }

    let mut pairs: Vec<(String, String)> = Vec::new();
    for secret_name in &transform.secrets {
        let info = state
            .db
            .get_secret_info(project_id, secret_name)
            .await?
            .ok_or_else(|| {
                ApiError::BadRequest(format!(
                    "Transform declares secret '{}' but it is not set for this project",
                    secret_name
                ))
            })?;
        pairs.push((secret_name.clone(), info.version_id.to_string()));
    }

    let refs: Vec<(&str, &str)> = pairs
        .iter()
        .map(|(n, v)| (n.as_str(), v.as_str()))
        .collect();
    Ok(ozzy_core::hash::secrets_hash(&refs))
}

/// Decrypt a secret value using AES-256-GCM.
fn decrypt_secret(encrypted: &[u8], key: &[u8]) -> Result<String, anyhow::Error> {
    use aes_gcm::{Aes256Gcm, KeyInit, Nonce, aead::Aead};

    if key.len() != 32 {
        anyhow::bail!("Encryption key must be exactly 32 bytes, got {}", key.len());
    }
    if encrypted.len() < 12 {
        anyhow::bail!("Encrypted data too short (missing nonce)");
    }

    let (nonce_bytes, ciphertext) = encrypted.split_at(12);
    let cipher = Aes256Gcm::new(key.into());
    let nonce = Nonce::from_slice(nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| anyhow::anyhow!("Decryption failed: {}", e))?;

    String::from_utf8(plaintext).map_err(|e| anyhow::anyhow!("Secret is not valid UTF-8: {}", e))
}

/// List output files in a directory, sorted by name.
async fn list_output_files(dir: &std::path::Path) -> Result<Vec<std::path::PathBuf>, ApiError> {
    let mut entries = tokio::fs::read_dir(dir)
        .await
        .map_err(|e| ApiError::Internal(e.into()))?;
    let mut files = Vec::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| ApiError::Internal(e.into()))?
    {
        if entry
            .file_type()
            .await
            .map_err(|e| ApiError::Internal(e.into()))?
            .is_file()
        {
            files.push(entry.path());
        }
    }
    files.sort();
    Ok(files)
}

/// Select the primary output file from a sorted list.
///
/// Prefers files starting with "result" (matching CLI's `find_primary_output`),
/// falling back to the first file alphabetically.
fn find_primary_output(files: &[std::path::PathBuf]) -> Option<&std::path::PathBuf> {
    // Prefer "result.*" files (standard runner output)
    for f in files {
        if let Some(name) = f.file_name().and_then(|n| n.to_str()) {
            if name.starts_with("result") {
                return Some(f);
            }
        }
    }
    // Fall back to first file alphabetically (already sorted)
    files.first()
}

/// Infer content type from file extension.
fn infer_output_content_type(path: &std::path::Path) -> String {
    match path.extension().and_then(|e| e.to_str()) {
        Some("parquet") => "application/vnd.apache.parquet".to_string(),
        Some("json") => "application/json".to_string(),
        Some("csv") => "text/csv".to_string(),
        Some("txt") => "text/plain".to_string(),
        Some("png") => "image/png".to_string(),
        Some("jpg") | Some("jpeg") => "image/jpeg".to_string(),
        Some("svg") => "image/svg+xml".to_string(),
        _ => "application/octet-stream".to_string(),
    }
}

/// Convert a MIME content type to a file extension for storage.
fn content_type_to_extension(content_type: &str) -> String {
    match content_type {
        "application/vnd.apache.parquet" => "parquet".to_string(),
        "application/json" => "json".to_string(),
        "text/csv" => "csv".to_string(),
        "text/plain" => "txt".to_string(),
        "image/png" => "png".to_string(),
        "image/jpeg" => "jpg".to_string(),
        "image/svg+xml" => "svg".to_string(),
        _ => "bin".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ozzy_core::toml_spec::{EdgeDef, EndpointDef, EndpointParamDef, NodeDef};

    fn make_test_endpoint() -> EndpointDef {
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
            description: Some("test endpoint".to_string()),
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

    #[test]
    fn test_build_execution_order() {
        let endpoint = make_test_endpoint();
        let order = build_execution_order(&endpoint).unwrap();
        assert_eq!(order.len(), 2);
        // step1 must come before step2
        let pos1 = order.iter().position(|n| n == "step1").unwrap();
        let pos2 = order.iter().position(|n| n == "step2").unwrap();
        assert!(pos1 < pos2);
    }

    #[test]
    fn test_find_terminal_node() {
        let endpoint = make_test_endpoint();
        // step2 is the terminal node (step1 → step2, no edges from step2 to other nodes)
        let terminal = find_terminal_node(&endpoint).unwrap();
        assert_eq!(terminal, "step2");
    }

    #[test]
    fn test_build_execution_order_single_node() {
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
                from: "data:input".to_string(),
                to: "only.data".to_string(),
            }],
        };
        let order = build_execution_order(&endpoint).unwrap();
        assert_eq!(order, vec!["only"]);
    }

    #[test]
    fn test_build_edge_map() {
        let endpoint = make_test_endpoint();
        let map = build_edge_map(&endpoint);

        assert_eq!(map.get("step1").unwrap().len(), 1);
        assert_eq!(map.get("step1").unwrap()[0], ("data", "data:raw"));

        assert_eq!(map.get("step2").unwrap().len(), 1);
        assert_eq!(map.get("step2").unwrap()[0], ("input", "step1"));
    }

    #[test]
    fn test_validate_params_with_defaults() {
        let mut params = HashMap::new();
        params.insert(
            "threshold".to_string(),
            EndpointParamDef {
                type_: "float".to_string(),
                default: Some(serde_json::json!(10.0)),
                binds: "step1.threshold".to_string(),
                min: Some(0.0),
                max: Some(100.0),
                enum_values: None,
                description: None,
            },
        );
        let endpoint = EndpointDef {
            description: None,
            params,
            nodes: HashMap::new(),
            edges: vec![],
        };

        // No params provided — should use default
        let resolved = validate_and_resolve_params(&endpoint, &HashMap::new()).unwrap();
        assert_eq!(resolved.get("threshold").unwrap(), 10.0);
    }

    #[test]
    fn test_validate_params_override() {
        let mut params = HashMap::new();
        params.insert(
            "threshold".to_string(),
            EndpointParamDef {
                type_: "float".to_string(),
                default: Some(serde_json::json!(10.0)),
                binds: "step1.threshold".to_string(),
                min: Some(0.0),
                max: Some(100.0),
                enum_values: None,
                description: None,
            },
        );
        let endpoint = EndpointDef {
            description: None,
            params,
            nodes: HashMap::new(),
            edges: vec![],
        };

        let mut consumer = HashMap::new();
        consumer.insert("threshold".to_string(), serde_json::json!(50.0));
        let resolved = validate_and_resolve_params(&endpoint, &consumer).unwrap();
        assert_eq!(resolved.get("threshold").unwrap(), 50.0);
    }

    #[test]
    fn test_validate_params_out_of_range() {
        let mut params = HashMap::new();
        params.insert(
            "threshold".to_string(),
            EndpointParamDef {
                type_: "float".to_string(),
                default: None,
                binds: "step1.threshold".to_string(),
                min: Some(0.0),
                max: Some(20.0),
                enum_values: None,
                description: None,
            },
        );
        let endpoint = EndpointDef {
            description: None,
            params,
            nodes: HashMap::new(),
            edges: vec![],
        };

        let mut consumer = HashMap::new();
        consumer.insert("threshold".to_string(), serde_json::json!(100.0));
        let err = validate_and_resolve_params(&endpoint, &consumer);
        assert!(err.is_err());
    }

    #[test]
    fn test_validate_params_missing_required() {
        let mut params = HashMap::new();
        params.insert(
            "threshold".to_string(),
            EndpointParamDef {
                type_: "float".to_string(),
                default: None,
                binds: "step1.threshold".to_string(),
                min: None,
                max: None,
                enum_values: None,
                description: None,
            },
        );
        let endpoint = EndpointDef {
            description: None,
            params,
            nodes: HashMap::new(),
            edges: vec![],
        };

        let err = validate_and_resolve_params(&endpoint, &HashMap::new());
        assert!(err.is_err());
    }

    #[test]
    fn test_infer_output_content_type() {
        assert_eq!(
            infer_output_content_type(std::path::Path::new("result.parquet")),
            "application/vnd.apache.parquet"
        );
        assert_eq!(
            infer_output_content_type(std::path::Path::new("result.json")),
            "application/json"
        );
        assert_eq!(
            infer_output_content_type(std::path::Path::new("result.csv")),
            "text/csv"
        );
        assert_eq!(
            infer_output_content_type(std::path::Path::new("result.png")),
            "image/png"
        );
        assert_eq!(
            infer_output_content_type(std::path::Path::new("result.bin")),
            "application/octet-stream"
        );
    }

    #[test]
    fn test_content_type_to_extension() {
        assert_eq!(
            content_type_to_extension("application/vnd.apache.parquet"),
            "parquet"
        );
        assert_eq!(content_type_to_extension("application/json"), "json");
        assert_eq!(content_type_to_extension("text/csv"), "csv");
        assert_eq!(content_type_to_extension("image/png"), "png");
        assert_eq!(content_type_to_extension("application/octet-stream"), "bin");
    }

    #[test]
    fn test_resolve_node_params_with_binds() {
        let node = NodeDef {
            transform: "t".to_string(),
            params: {
                let mut p = HashMap::new();
                p.insert("static_key".to_string(), serde_json::json!("static_val"));
                p
            },
            machine: None,
        };

        let endpoint = EndpointDef {
            description: None,
            nodes: {
                let mut n = HashMap::new();
                n.insert("mynode".to_string(), node.clone());
                n
            },
            params: {
                let mut p = HashMap::new();
                p.insert(
                    "user_threshold".to_string(),
                    EndpointParamDef {
                        type_: "float".to_string(),
                        default: Some(serde_json::json!(10.0)),
                        binds: "mynode.threshold".to_string(),
                        min: None,
                        max: None,
                        enum_values: None,
                        description: None,
                    },
                );
                p.insert(
                    "other_param".to_string(),
                    EndpointParamDef {
                        type_: "string".to_string(),
                        default: Some(serde_json::json!("x")),
                        binds: "othernode.value".to_string(),
                        min: None,
                        max: None,
                        enum_values: None,
                        description: None,
                    },
                );
                p
            },
            edges: vec![],
        };

        let endpoint_params = serde_json::json!({
            "user_threshold": 12.5,
            "other_param": "csv",
        });

        let resolved = resolve_node_params("mynode", &node, &endpoint, &endpoint_params);
        // Static param preserved
        assert_eq!(resolved.get("static_key").unwrap(), "static_val");
        // Bound param mapped: user_threshold -> threshold (via binds "mynode.threshold")
        assert_eq!(resolved.get("threshold").unwrap(), 12.5);
        // other_param bound to "othernode.value" should NOT appear on "mynode"
        assert!(resolved.get("other_param").is_none());
        assert!(resolved.get("value").is_none());
    }

    #[test]
    fn test_coerce_param_value_float() {
        let v = serde_json::json!("12.5");
        let c = coerce_param_value(&v, "float");
        assert_eq!(c, serde_json::json!(12.5));
    }

    #[test]
    fn test_coerce_param_value_int() {
        let v = serde_json::json!("42");
        let c = coerce_param_value(&v, "int");
        assert_eq!(c, serde_json::json!(42));
    }

    #[test]
    fn test_coerce_param_value_bool() {
        assert_eq!(
            coerce_param_value(&serde_json::json!("true"), "bool"),
            serde_json::json!(true)
        );
        assert_eq!(
            coerce_param_value(&serde_json::json!("false"), "bool"),
            serde_json::json!(false)
        );
    }

    #[test]
    fn test_coerce_param_value_string_passthrough() {
        let v = serde_json::json!("hello");
        let c = coerce_param_value(&v, "string");
        assert_eq!(c, serde_json::json!("hello"));
    }

    #[test]
    fn test_coerce_param_value_already_typed() {
        // Already a number — should pass through unchanged
        let v = serde_json::json!(12.5);
        let c = coerce_param_value(&v, "float");
        assert_eq!(c, serde_json::json!(12.5));
    }

    #[test]
    fn test_validate_rejects_invalid_float() {
        // "not-a-number" fails coercion and should be rejected by validation
        let mut params = HashMap::new();
        params.insert(
            "threshold".to_string(),
            EndpointParamDef {
                type_: "float".to_string(),
                default: None,
                binds: "step1.threshold".to_string(),
                min: None,
                max: None,
                enum_values: None,
                description: None,
            },
        );
        let endpoint = EndpointDef {
            description: None,
            params,
            nodes: HashMap::new(),
            edges: vec![],
        };

        let mut consumer = HashMap::new();
        consumer.insert("threshold".to_string(), serde_json::json!("not-a-number"));
        let err = validate_and_resolve_params(&endpoint, &consumer);
        assert!(err.is_err());
    }

    #[test]
    fn test_validate_rejects_invalid_bool() {
        let mut params = HashMap::new();
        params.insert(
            "flag".to_string(),
            EndpointParamDef {
                type_: "bool".to_string(),
                default: None,
                binds: "step1.flag".to_string(),
                min: None,
                max: None,
                enum_values: None,
                description: None,
            },
        );
        let endpoint = EndpointDef {
            description: None,
            params,
            nodes: HashMap::new(),
            edges: vec![],
        };

        let mut consumer = HashMap::new();
        consumer.insert("flag".to_string(), serde_json::json!("maybe"));
        let err = validate_and_resolve_params(&endpoint, &consumer);
        assert!(err.is_err());
    }

    #[test]
    fn test_validate_accepts_coerced_string_float() {
        // "12.5" coerces to 12.5 for float type — should pass
        let mut params = HashMap::new();
        params.insert(
            "threshold".to_string(),
            EndpointParamDef {
                type_: "float".to_string(),
                default: None,
                binds: "step1.threshold".to_string(),
                min: Some(0.0),
                max: Some(100.0),
                enum_values: None,
                description: None,
            },
        );
        let endpoint = EndpointDef {
            description: None,
            params,
            nodes: HashMap::new(),
            edges: vec![],
        };

        let mut consumer = HashMap::new();
        consumer.insert("threshold".to_string(), serde_json::json!("12.5"));
        let resolved = validate_and_resolve_params(&endpoint, &consumer).unwrap();
        assert_eq!(resolved.get("threshold").unwrap(), 12.5);
    }

    #[test]
    fn test_validate_rejects_invalid_default_value() {
        let mut params = HashMap::new();
        params.insert(
            "threshold".to_string(),
            EndpointParamDef {
                type_: "float".to_string(),
                description: None,
                default: Some(serde_json::json!(200.0)), // exceeds max
                binds: "node.threshold".to_string(),
                min: None,
                max: Some(100.0),
                enum_values: None,
            },
        );
        let endpoint = EndpointDef {
            description: None,
            params,
            nodes: HashMap::new(),
            edges: vec![],
        };
        // Should reject the default that violates max constraint
        let err = validate_and_resolve_params(&endpoint, &HashMap::new());
        assert!(
            err.is_err(),
            "Default value exceeding max should be rejected"
        );
    }
}
