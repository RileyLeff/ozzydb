//! Fetch endpoint — resolves and executes an endpoint's DAG asynchronously.
//!
//! `POST /v1/fetch/{owner}/{project}/{endpoint}` creates a job for endpoint execution.
//! Returns 202 + job_id for async execution, or 200 if all nodes are cached.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::auth::ApiError;
use crate::AppState;
use crate::auth::middleware::MaybeAuthUser;
use crate::compute::orchestrator::NodeOutput;
use crate::registry::{
    RegistrySnapshot, RuntimeEnvironmentDef, RuntimeTransformDef,
    load_published_project_revision_by_commit,
};
use crate::verification::ensure_conformance_verified;

/// Build the fetch router.
pub fn router() -> Router<AppState> {
    Router::new().route("/{owner}/{slug}/{endpoint}", post(fetch_endpoint))
}

#[derive(Debug, Deserialize)]
struct FetchRequest {
    #[serde(rename = "ref")]
    ref_name: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    format: Option<String>,
    #[serde(default)]
    params: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    inputs: BTreeMap<String, Uuid>,
}

#[derive(Debug, Serialize)]
struct FetchResponse {
    job_id: Uuid,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_hash: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedEndpointInput {
    pub artifact: crate::db::v4::StoredArtifact,
    pub type_ref: ozzy_types::syntax::TypeRefExpr,
}

/// Fetch endpoint: validate, check cache, create job, return immediately.
async fn fetch_endpoint(
    State(state): State<AppState>,
    Path((owner, slug, endpoint_name)): Path<(String, String, String)>,
    auth: MaybeAuthUser,
    Json(request): Json<FetchRequest>,
) -> Result<Response, ApiError> {
    // ── 1. Resolve project → ref → commit ───────────────────────
    let project =
        state.db.get_project(&owner, &slug).await?.ok_or_else(|| {
            ApiError::not_found(format!("Project '{}/{}' not found", owner, slug))
        })?;

    super::access::enforce_read_access(&state, &project, &owner, &slug, &auth).await?;

    let commit = if let Some(ref ref_name) = request.ref_name {
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

    let published =
        load_published_project_revision_by_commit(&state.db, &state.registry_snapshots, commit.id)
            .await
            .map_err(|e| ApiError::Internal(e.into()))?;

    // ── 2. Resolve endpoint from the published project revision ───────────
    let endpoint_def = published
        .endpoints
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

    // ── 4. Validate consumer params ─────────────────────────────
    let request_params = request.params.into_iter().collect::<HashMap<_, _>>();
    let resolved_params = validate_and_resolve_params(endpoint_def, &request_params)?;
    let resolved_inputs = validate_and_resolve_endpoint_inputs(
        &state,
        project.id,
        published.snapshot.as_ref(),
        endpoint_def,
        &request.inputs,
    )
    .await?;

    // Compute canonical params hash for dedup
    let canonical_params =
        serde_json::to_string(&resolved_params).map_err(|e| ApiError::Internal(e.into()))?;
    let params_hash = ozzy_core::hash::blake3_hash(canonical_params.as_bytes());
    let input_bindings =
        build_job_input_bindings(&resolved_inputs).map_err(|e| ApiError::Internal(e.into()))?;
    let canonical_input_bindings =
        serde_json::to_string(&input_bindings).map_err(|e| ApiError::Internal(e.into()))?;
    let input_bindings_hash = ozzy_core::hash::blake3_hash(canonical_input_bindings.as_bytes());

    // ── 5. Check dedup (return existing active job) ─────────────
    if let Some(existing) = state
        .db
        .find_active_job(
            project.id,
            &endpoint_name,
            commit.id,
            &params_hash,
            &input_bindings_hash,
        )
        .await?
    {
        return Ok((
            StatusCode::OK,
            Json(FetchResponse {
                job_id: existing.id,
                status: existing.status.clone(),
                output_url: existing
                    .output_hash
                    .as_ref()
                    .map(|_| format!("/api/v1/jobs/{}/output", existing.id)),
                output_hash: existing.output_hash,
            }),
        )
            .into_response());
    }

    // ── 6. Build execution order + cache-hit fast path ──────────
    let exec_order = build_execution_order(endpoint_def)?;

    let (all_cached, node_outputs) = check_all_node_caches(
        &state,
        &commit,
        published.snapshot.as_ref(),
        endpoint_def,
        &published.runtime.transforms,
        &resolved_params,
        &resolved_inputs,
        &exec_order,
        project.id,
    )
    .await?;

    // Build initial node status
    let node_status_map: serde_json::Value = exec_order
        .iter()
        .map(|n| {
            let status = if node_outputs.contains_key(n) {
                "done"
            } else {
                "queued"
            };
            (n.clone(), serde_json::json!(status))
        })
        .collect::<serde_json::Map<_, _>>()
        .into();

    let auth_user_id = auth.user.as_ref().map(|u| u.id);

    if all_cached {
        // ── Fast path: all nodes cached → return 200 immediately ──
        let terminal = find_terminal_node(endpoint_def)?;
        let output = node_outputs.get(terminal).ok_or_else(|| {
            ApiError::Internal(anyhow::anyhow!(
                "Terminal node '{}' has no cached output",
                terminal
            ))
        })?;

        let job = state
            .db
            .create_job(
                project.id,
                &endpoint_name,
                commit.id,
                &resolved_params,
                &params_hash,
                &input_bindings,
                &input_bindings_hash,
                &node_status_map,
                auth_user_id,
            )
            .await
            .map_err(ApiError::Internal)?;

        state
            .db
            .set_job_output(job.id, &output.output_hash, &output.content_type)
            .await
            .map_err(ApiError::Internal)?;

        tracing::info!(
            "Cache hit fast path for {}/{}/{}: job {}",
            owner,
            slug,
            endpoint_name,
            job.id
        );

        return Ok((
            StatusCode::OK,
            Json(FetchResponse {
                job_id: job.id,
                status: "done".to_string(),
                output_url: Some(format!("/api/v1/jobs/{}/output", job.id)),
                output_hash: Some(output.output_hash.clone()),
            }),
        )
            .into_response());
    }

    // ── 7. Rate limit check (only for non-cached execution) ─────
    // NOTE: This is an advisory/soft limit — concurrent requests can race past
    // the check before job creation (TOCTOU). This is standard for rate limiters
    // and acceptable since the limits are for resource protection, not billing.
    if let Some(user_id) = auth_user_id {
        if let Err(e) =
            crate::compute::rate_limit::check_limits(&state.db, &state.config.rate_limit, user_id)
                .await
        {
            return Err(ApiError::TooManyRequests(e.to_string()));
        }
    } else {
        // Anonymous users: check global limit only
        if state.config.rate_limit.global_max_concurrent > 0 {
            let global_active = state
                .db
                .count_active_jobs_global()
                .await
                .map_err(ApiError::Internal)?;
            if global_active >= state.config.rate_limit.global_max_concurrent as i64 {
                return Err(ApiError::TooManyRequests(format!(
                    "Global concurrent job limit exceeded ({}/{})",
                    global_active, state.config.rate_limit.global_max_concurrent
                )));
            }
        }
    }

    // ── 9. Create queued job + spawn background execution ────────
    let job = state
        .db
        .create_job(
            project.id,
            &endpoint_name,
            commit.id,
            &resolved_params,
            &params_hash,
            &input_bindings,
            &input_bindings_hash,
            &node_status_map,
            auth_user_id,
        )
        .await
        .map_err(ApiError::Internal)?;

    let job_id = job.id;
    let state_clone = state.clone();
    tokio::spawn(crate::compute::orchestrator::run_job(state_clone, job_id));

    tracing::info!(
        "Created job {} for {}/{}/{}",
        job_id,
        owner,
        slug,
        endpoint_name
    );

    Ok((
        StatusCode::ACCEPTED,
        Json(FetchResponse {
            job_id,
            status: "queued".to_string(),
            output_url: None,
            output_hash: None,
        }),
    )
        .into_response())
}

// ── Cache check ───────────────────────────────────────────────────

/// Check if all nodes in the DAG have cached outputs.
///
/// Iterates through nodes in topological order, computing materialized hashes
/// and checking the cache. Returns (all_cached, node_outputs_so_far).
/// On any cache miss, stops early and returns false.
async fn check_all_node_caches(
    state: &AppState,
    commit: &crate::db::Commit,
    snapshot: &RegistrySnapshot,
    endpoint_def: &ozzy_core::toml_spec::EndpointDef,
    transforms: &HashMap<String, RuntimeTransformDef>,
    resolved_params: &serde_json::Value,
    endpoint_inputs: &BTreeMap<String, ResolvedEndpointInput>,
    exec_order: &[String],
    project_id: Uuid,
) -> Result<(bool, HashMap<String, NodeOutput>), ApiError> {
    let source_dir =
        if endpoint_requires_source_code(endpoint_def, transforms).map_err(ApiError::Internal)? {
            Some(
                retrieve_source_code(state, commit)
                    .await
                    .map_err(ApiError::Internal)?,
            )
        } else {
            None
        };
    let edge_map = build_edge_map(endpoint_def);
    let mut node_outputs: HashMap<String, NodeOutput> = HashMap::new();

    for node_name in exec_order {
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
        let bound_inputs = edge_map
            .get(node_name.as_str())
            .into_iter()
            .flatten()
            .map(|(name, _)| *name);
        validate_node_input_bindings(node_name, transform_def, bound_inputs)
            .map_err(ApiError::Internal)?;

        let mat_hash = compute_materialized_hash(
            state,
            node_name,
            node_def,
            transform_def,
            endpoint_def,
            resolved_params,
            endpoint_inputs,
            &edge_map,
            &node_outputs,
            &source_dir,
            project_id,
        )
        .await?;

        if let Some(cached) = state.db.get_materialized_cache(&mat_hash).await? {
            let cached_artifact = state
                .db
                .get_v4_artifact(cached.output_artifact_id)
                .await
                .map_err(|e| ApiError::Internal(e.into()))?
                .ok_or_else(|| {
                    ApiError::Internal(anyhow::anyhow!(
                        "Materialized cache '{}' references missing artifact '{}'",
                        mat_hash,
                        cached.output_artifact_id
                    ))
                })?;
            let (_, output_port) = single_output_port(&node_def.transform, transform_def)
                .map_err(ApiError::Internal)?;
            let (_, output_type) = snapshot
                .resolve_type_ref(&output_port.ty)
                .map_err(|e| ApiError::Internal(e.into()))?;
            let conformance = state
                .db
                .get_v4_conformance_record(cached_artifact.id, output_type.id)
                .await
                .map_err(|e| ApiError::Internal(e.into()))?
                .ok_or_else(|| {
                    ApiError::Internal(anyhow::anyhow!(
                        "Materialized cache '{}' references artifact '{}' without output conformance for type '{}'",
                        mat_hash,
                        cached_artifact.id,
                        output_port.ty.name
                    ))
                })?;
            let conformance = ensure_conformance_verified(
                state,
                snapshot,
                &cached_artifact,
                &conformance,
                &output_port.ty,
            )
            .await
            .map_err(|e| ApiError::Internal(e.into()))?;
            if conformance.status == "rejected" {
                return Err(ApiError::Internal(anyhow::anyhow!(
                    "Materialized cache '{}' references rejected artifact '{}' for output type '{}'",
                    mat_hash,
                    cached_artifact.id,
                    output_port.ty.name
                )));
            }
            state.db.touch_materialized_cache(&mat_hash).await?;
            node_outputs.insert(
                node_name.clone(),
                NodeOutput {
                    materialized_hash: mat_hash,
                    output_hash: cached.output_hash,
                    content_type: cached.output_content_type,
                    byte_size: cached.output_byte_size,
                    cache_hit: true,
                    artifact_id: cached.output_artifact_id,
                },
            );
        } else {
            return Ok((false, node_outputs));
        }
    }

    Ok((true, node_outputs))
}

/// Compute the materialized hash for a single node.
///
/// Resolves bound input artifacts and combines them with transform, environment,
/// source, params, and secret identity into the v4 cache key.
async fn compute_materialized_hash(
    state: &AppState,
    node_name: &str,
    node_def: &ozzy_core::toml_spec::NodeDef,
    transform_def: &RuntimeTransformDef,
    endpoint_def: &ozzy_core::toml_spec::EndpointDef,
    resolved_params: &serde_json::Value,
    endpoint_inputs: &BTreeMap<String, ResolvedEndpointInput>,
    edge_map: &HashMap<&str, Vec<(&str, &str)>>,
    node_outputs: &HashMap<String, NodeOutput>,
    source_dir: &Option<tempfile::TempDir>,
    project_id: Uuid,
) -> Result<String, ApiError> {
    // Resolve inputs
    let mut input_artifact_ids: Vec<(&str, String)> = Vec::new();
    let empty_vec = vec![];
    let edges_for_node = edge_map.get(node_name).unwrap_or(&empty_vec);
    for (input_name, source) in edges_for_node {
        let artifact_id = resolve_edge_source(source, endpoint_inputs, node_outputs).await?;
        input_artifact_ids.push((input_name, artifact_id));
    }

    // Resolve node params
    let node_params = resolve_node_params(node_name, node_def, endpoint_def, resolved_params);
    let params_hash = ozzy_core::hash::blake3_hash(
        serde_json::to_string(&node_params)
            .map_err(|e| ApiError::Internal(e.into()))?
            .as_bytes(),
    );

    // Resolve secrets hash
    let secrets_hash = resolve_secrets_hash(state, project_id, transform_def).await?;

    // Compute source_hash
    let source_hash = compute_source_hash(transform_def, node_def, source_dir)?;

    let input_refs: Vec<(&str, &str)> = input_artifact_ids
        .iter()
        .map(|(name, artifact_id)| (*name, artifact_id.as_str()))
        .collect();

    Ok(ozzy_core::hash::materialized_hash(
        &input_refs,
        &transform_def.row_id.to_string(),
        &transform_def.environment.row_id.to_string(),
        &source_hash,
        &params_hash,
        secrets_hash.as_deref(),
    ))
}

/// Compute source hash for a transform.
fn compute_source_hash(
    transform_def: &RuntimeTransformDef,
    node_def: &ozzy_core::toml_spec::NodeDef,
    source_dir: &Option<tempfile::TempDir>,
) -> Result<String, ApiError> {
    if let Some(source) = &transform_def.source {
        let (_file_path, _func) = crate::runners::parse_source_ref(source).ok_or_else(|| {
            ApiError::Internal(anyhow::anyhow!(
                "Invalid source ref '{}' for transform '{}'",
                source,
                node_def.transform
            ))
        })?;
        let file_path_str = source.split(':').next().unwrap_or(source);
        let sd = source_dir.as_ref().ok_or_else(|| {
            ApiError::Internal(anyhow::anyhow!(
                "Source transform '{}' requires extracted source code, but none was loaded",
                node_def.transform
            ))
        })?;
        let source_root = sd.path().canonicalize().map_err(|e| {
            ApiError::Internal(anyhow::anyhow!("Failed to canonicalize source dir: {}", e))
        })?;
        let full_path = sd.path().join(file_path_str);
        let canonical = full_path.canonicalize().map_err(|e| {
            ApiError::Internal(anyhow::anyhow!(
                "Failed to canonicalize source path '{}' for transform '{}': {}",
                file_path_str,
                node_def.transform,
                e
            ))
        })?;
        if !canonical.starts_with(&source_root) {
            return Err(ApiError::BadRequest(format!(
                "Source path '{}' escapes source directory",
                file_path_str,
            )));
        }
        let bytes = std::fs::read(&canonical).map_err(|e| {
            ApiError::Internal(anyhow::anyhow!(
                "Failed to read source file '{}' for transform '{}': {}",
                file_path_str,
                node_def.transform,
                e
            ))
        })?;
        Ok(ozzy_core::hash::blake3_hash(&bytes))
    } else if let Some(command) = &transform_def.command {
        Ok(ozzy_core::hash::blake3_hash(command.as_bytes()))
    } else {
        Err(ApiError::Internal(anyhow::anyhow!(
            "Transform '{}' has neither source nor command",
            node_def.transform
        )))
    }
}

// ── Background job execution is now in compute::orchestrator ──────

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
            let coerced = coerce_param_value(v, &param_def.type_)?;
            validate_param_value(name, &coerced, param_def)?;
            coerced
        } else if let Some(default) = &param_def.default {
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

pub(crate) async fn validate_and_resolve_endpoint_inputs(
    state: &AppState,
    project_id: Uuid,
    snapshot: &RegistrySnapshot,
    endpoint: &ozzy_core::toml_spec::EndpointDef,
    requested_inputs: &BTreeMap<String, Uuid>,
) -> Result<BTreeMap<String, ResolvedEndpointInput>, ApiError> {
    let expected = endpoint
        .inputs
        .ports
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    let actual = requested_inputs.keys().cloned().collect::<BTreeSet<_>>();

    if actual != expected {
        let missing = expected.difference(&actual).cloned().collect::<Vec<_>>();
        let unexpected = actual.difference(&expected).cloned().collect::<Vec<_>>();

        let mut problems = Vec::new();
        if !missing.is_empty() {
            problems.push(format!("missing inputs {:?}", missing));
        }
        if !unexpected.is_empty() {
            problems.push(format!("unexpected inputs {:?}", unexpected));
        }

        return Err(ApiError::BadRequest(format!(
            "Endpoint input bindings are invalid: {}",
            problems.join(", ")
        )));
    }

    let mut resolved = BTreeMap::new();
    for (input_name, port) in &endpoint.inputs.ports {
        let artifact_id = *requested_inputs.get(input_name).ok_or_else(|| {
            ApiError::BadRequest(format!(
                "Endpoint input '{}' is missing from the fetch request",
                input_name
            ))
        })?;

        let artifact = state
            .db
            .get_v4_artifact(artifact_id)
            .await
            .map_err(|e| ApiError::Internal(e.into()))?
            .ok_or_else(|| {
                ApiError::BadRequest(format!(
                    "Artifact '{}' bound to endpoint input '{}' does not exist",
                    artifact_id, input_name
                ))
            })?;

        if artifact.project_id != project_id {
            return Err(ApiError::BadRequest(format!(
                "Artifact '{}' cannot be bound to endpoint input '{}' because it belongs to another project",
                artifact.id, input_name
            )));
        }

        let (_, stored_type) = snapshot
            .resolve_type_ref(&port.ty)
            .map_err(|e| ApiError::Internal(e.into()))?;
        let conformance = state
            .db
            .get_v4_conformance_record(artifact.id, stored_type.id)
            .await
            .map_err(|e| ApiError::Internal(e.into()))?
            .ok_or_else(|| {
                ApiError::BadRequest(format!(
                    "Artifact '{}' has no conformance record for required endpoint input type '{}'",
                    artifact.id, port.ty.name
                ))
            })?;

        let conformance =
            ensure_conformance_verified(state, snapshot, &artifact, &conformance, &port.ty)
                .await
                .map_err(|e| ApiError::Internal(e.into()))?;

        if conformance.status == "rejected" {
            return Err(ApiError::BadRequest(format!(
                "Artifact '{}' was rejected for required endpoint input type '{}'",
                artifact.id, port.ty.name
            )));
        }

        resolved.insert(
            input_name.clone(),
            ResolvedEndpointInput {
                artifact,
                type_ref: port.ty.clone(),
            },
        );
    }

    Ok(resolved)
}

fn build_job_input_bindings(
    inputs: &BTreeMap<String, ResolvedEndpointInput>,
) -> Result<serde_json::Value, anyhow::Error> {
    let object = inputs
        .iter()
        .map(|(name, binding)| {
            Ok((
                name.clone(),
                serde_json::Value::String(binding.artifact.id.to_string()),
            ))
        })
        .collect::<Result<serde_json::Map<String, serde_json::Value>, anyhow::Error>>()?;
    Ok(serde_json::Value::Object(object))
}

/// Coerce a query-string parameter value to the declared type.
fn coerce_param_value(
    value: &serde_json::Value,
    declared_type: &str,
) -> Result<serde_json::Value, ApiError> {
    if let serde_json::Value::String(s) = value {
        match declared_type {
            "float" | "number" => {
                let n = s.parse::<f64>().map_err(|_| {
                    ApiError::BadRequest(format!(
                        "Parameter value '{}' is not a valid {}",
                        s, declared_type
                    ))
                })?;
                return Ok(serde_json::Value::from(n));
            }
            "int" | "integer" => {
                let n = s.parse::<i64>().map_err(|_| {
                    ApiError::BadRequest(format!(
                        "Parameter value '{}' is not a valid {}",
                        s, declared_type
                    ))
                })?;
                return Ok(serde_json::Value::from(n));
            }
            "bool" | "boolean" => match s.as_str() {
                "true" | "1" | "yes" => return Ok(serde_json::Value::Bool(true)),
                "false" | "0" | "no" => return Ok(serde_json::Value::Bool(false)),
                _ => {
                    return Err(ApiError::BadRequest(format!(
                        "Parameter value '{}' is not a valid {}",
                        s, declared_type
                    )));
                }
            },
            _ => {}
        }
    }
    Ok(value.clone())
}

/// Validate a parameter value against its definition.
fn validate_param_value(
    name: &str,
    value: &serde_json::Value,
    param_def: &ozzy_core::toml_spec::EndpointParamDef,
) -> Result<(), ApiError> {
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
        _ => {}
    }

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
    build_execution_order_inner(endpoint).map_err(|e| ApiError::Internal(e))
}

fn build_execution_order_inner(
    endpoint: &ozzy_core::toml_spec::EndpointDef,
) -> Result<Vec<String>, anyhow::Error> {
    let nodes: HashSet<&str> = endpoint.nodes.keys().map(|s| s.as_str()).collect();

    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();

    for node in &nodes {
        in_degree.insert(node, 0);
        adj.insert(node, Vec::new());
    }

    for edge in &endpoint.edges {
        let to_node = edge.to.split('.').next().unwrap_or(&edge.to);
        if nodes.contains(edge.from.as_str()) {
            adj.get_mut(edge.from.as_str()).map(|v| v.push(to_node));
            *in_degree.entry(to_node).or_insert(0) += 1;
        }
    }

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
        anyhow::bail!("Cycle detected in endpoint DAG");
    }

    Ok(order)
}

/// Find the single terminal (sink) node of an endpoint DAG.
fn find_terminal_node(endpoint: &ozzy_core::toml_spec::EndpointDef) -> Result<&str, ApiError> {
    find_terminal_node_inner(endpoint).map_err(ApiError::Internal)
}

pub(crate) fn find_terminal_node_anyhow(
    endpoint: &ozzy_core::toml_spec::EndpointDef,
) -> Result<&str, anyhow::Error> {
    find_terminal_node_inner(endpoint)
}

fn find_terminal_node_inner(
    endpoint: &ozzy_core::toml_spec::EndpointDef,
) -> Result<&str, anyhow::Error> {
    let node_names: HashSet<&str> = endpoint.nodes.keys().map(|s| s.as_str()).collect();
    let mut has_outgoing: HashSet<&str> = HashSet::new();

    for edge in &endpoint.edges {
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
        0 => anyhow::bail!("Endpoint has no terminal node"),
        1 => Ok(terminals[0]),
        _ => {
            let mut names: Vec<&str> = terminals;
            names.sort();
            anyhow::bail!("Endpoint has multiple terminal nodes: {:?}", names)
        }
    }
}

pub(crate) fn single_output_port<'a>(
    transform_name: &str,
    transform_def: &'a RuntimeTransformDef,
) -> Result<(&'a str, &'a ozzy_types::ports::TypedPort), anyhow::Error> {
    match transform_def.outputs.ports.iter().next() {
        Some((name, port)) if transform_def.outputs.ports.len() == 1 => Ok((name.as_str(), port)),
        Some(_) => anyhow::bail!(
            "Transform '{}' has multiple outputs at runtime; explicit output-port edges are not implemented yet",
            transform_name
        ),
        None => anyhow::bail!(
            "Transform '{}' has no output ports at runtime",
            transform_name
        ),
    }
}

/// Build a map from node_name → [(input_name, edge_source)] for quick lookup.
pub(crate) fn build_edge_map<'a>(
    endpoint: &'a ozzy_core::toml_spec::EndpointDef,
) -> HashMap<&'a str, Vec<(&'a str, &'a str)>> {
    let mut map: HashMap<&str, Vec<(&str, &str)>> = HashMap::new();
    for edge in &endpoint.edges {
        if let Some((node_name, input_name)) = edge.to.split_once('.') {
            map.entry(node_name)
                .or_default()
                .push((input_name, edge.from.as_str()));
        }
    }
    map
}

/// Resolve an edge source to the bound artifact identity.
async fn resolve_edge_source(
    source: &str,
    endpoint_inputs: &BTreeMap<String, ResolvedEndpointInput>,
    node_outputs: &HashMap<String, NodeOutput>,
) -> Result<String, ApiError> {
    resolve_edge_source_inner(source, endpoint_inputs, node_outputs)
        .await
        .map_err(ApiError::Internal)
}

async fn resolve_edge_source_inner(
    source: &str,
    endpoint_inputs: &BTreeMap<String, ResolvedEndpointInput>,
    node_outputs: &HashMap<String, NodeOutput>,
) -> Result<String, anyhow::Error> {
    if let Some(input_name) = source.strip_prefix("input:") {
        let binding = endpoint_inputs.get(input_name).ok_or_else(|| {
            anyhow::anyhow!("Endpoint input '{}' is not bound for execution", input_name)
        })?;
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

/// Retrieve and extract the source tarball for a commit.
pub(crate) async fn retrieve_source_code(
    state: &AppState,
    commit: &crate::db::Commit,
) -> Result<tempfile::TempDir, anyhow::Error> {
    let source_storage =
        crate::storage::ContentStorage::from_config_with_prefix(&state.config, "source")
            .map_err(|e| anyhow::anyhow!("Failed to create source storage: {}", e))?;

    let tarball_bytes = source_storage
        .get(&commit.git_commit_sha, "tar.gz")
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "Source tarball not found for commit {} (sha {}): {}",
                commit.id,
                commit.git_commit_sha,
                e
            )
        })?;

    let tmp = tempfile::tempdir()
        .map_err(|e| anyhow::anyhow!("Failed to create temp dir for source extraction: {}", e))?;

    let tarball_path = tmp.path().join("source.tar.gz");
    tokio::fs::write(&tarball_path, &tarball_bytes)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to write source tarball: {}", e))?;

    let output = tokio::process::Command::new("tar")
        .arg("xzf")
        .arg(&tarball_path)
        .arg("-C")
        .arg(tmp.path())
        .arg("--strip-components=1")
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to run tar for source extraction: {}", e))?;

    if !output.status.success() {
        anyhow::bail!(
            "tar extraction failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    tokio::fs::remove_file(&tarball_path)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to remove temporary source tarball: {}", e))?;

    Ok(tmp)
}

pub(crate) fn validate_node_input_bindings<'a>(
    node_name: &str,
    transform_def: &RuntimeTransformDef,
    bound_inputs: impl IntoIterator<Item = &'a str>,
) -> Result<(), anyhow::Error> {
    let expected = transform_def
        .inputs
        .ports
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    let actual = bound_inputs
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();

    if actual == expected {
        return Ok(());
    }

    let missing = expected.difference(&actual).cloned().collect::<Vec<_>>();
    let unexpected = actual.difference(&expected).cloned().collect::<Vec<_>>();

    let mut problems = Vec::new();
    if !missing.is_empty() {
        problems.push(format!("missing inputs {:?}", missing));
    }
    if !unexpected.is_empty() {
        problems.push(format!("unexpected inputs {:?}", unexpected));
    }

    anyhow::bail!(
        "Node '{}' does not satisfy transform '{}' input ports: {}",
        node_name,
        transform_def.versioned_name,
        problems.join(", ")
    );
}

pub(crate) fn endpoint_requires_source_code(
    endpoint_def: &ozzy_core::toml_spec::EndpointDef,
    transforms: &HashMap<String, RuntimeTransformDef>,
) -> Result<bool, anyhow::Error> {
    for (node_name, node_def) in &endpoint_def.nodes {
        let transform_def = transforms.get(&node_def.transform).ok_or_else(|| {
            anyhow::anyhow!(
                "Transform '{}' not found for node '{}'",
                node_def.transform,
                node_name
            )
        })?;
        if transform_def.source.is_some() {
            return Ok(true);
        }
    }

    Ok(false)
}

pub(crate) async fn resolve_environment_image_anyhow(
    state: &AppState,
    env_def: &RuntimeEnvironmentDef,
) -> Result<Option<crate::db::EnvironmentImage>, anyhow::Error> {
    resolve_environment_image_inner(state, env_def).await
}

async fn resolve_environment_image_inner(
    state: &AppState,
    env_def: &RuntimeEnvironmentDef,
) -> Result<Option<crate::db::EnvironmentImage>, anyhow::Error> {
    match &env_def.definition {
        ozzy_core::toml_spec::PublishedEnvironmentDef::Prebuilt { image } => {
            let env_hash = crate::environments::hash::compute_env_hash(&env_def.definition);
            let env_image = crate::db::EnvironmentImage {
                id: Uuid::nil(),
                env_hash: env_hash.clone(),
                build_type: "prebuilt".to_string(),
                image_ref: image.clone(),
                base_image: None,
                build_log_r2_key: None,
                build_duration_ms: None,
                built_at: Some(chrono::Utc::now()),
                created_at: chrono::Utc::now(),
            };
            Ok(Some(env_image))
        }
        ozzy_core::toml_spec::PublishedEnvironmentDef::BaseLockfile { .. }
        | ozzy_core::toml_spec::PublishedEnvironmentDef::Dockerfile { .. } => {
            let env_hash = crate::environments::hash::compute_env_hash(&env_def.definition);
            let env_image = state.db.get_environment_image(&env_hash).await?;
            Ok(env_image)
        }
    }
}

/// Resolve params for a specific node.
pub(crate) fn resolve_node_params(
    node_name: &str,
    node_def: &ozzy_core::toml_spec::NodeDef,
    endpoint: &ozzy_core::toml_spec::EndpointDef,
    endpoint_params: &serde_json::Value,
) -> serde_json::Value {
    let mut params = serde_json::Map::new();

    for (key, value) in &node_def.params {
        params.insert(key.clone(), value.clone());
    }

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
    project_id: Uuid,
    transform: &RuntimeTransformDef,
) -> Result<Option<String>, ApiError> {
    resolve_secrets_hash_inner(state, project_id, transform)
        .await
        .map_err(ApiError::Internal)
}

async fn resolve_secrets_hash_inner(
    state: &AppState,
    project_id: Uuid,
    transform: &RuntimeTransformDef,
) -> Result<Option<String>, anyhow::Error> {
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
                anyhow::anyhow!(
                    "Transform declares secret '{}' but it is not set for this project",
                    secret_name
                )
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
pub(crate) fn decrypt_secret(encrypted: &[u8], key: &[u8]) -> Result<String, anyhow::Error> {
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

/// List output files in a directory (for handler use).
#[allow(dead_code)]
async fn list_output_files(dir: &std::path::Path) -> Result<Vec<std::path::PathBuf>, ApiError> {
    list_output_files_inner(dir)
        .await
        .map_err(ApiError::Internal)
}

/// List output files in a directory (for background task use).
pub(crate) async fn list_output_files_anyhow(
    dir: &std::path::Path,
) -> Result<Vec<std::path::PathBuf>, anyhow::Error> {
    list_output_files_inner(dir).await
}

async fn list_output_files_inner(
    dir: &std::path::Path,
) -> Result<Vec<std::path::PathBuf>, anyhow::Error> {
    let mut entries = tokio::fs::read_dir(dir).await?;
    let mut files = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        if entry.file_type().await?.is_file() {
            files.push(entry.path());
        }
    }
    files.sort();
    Ok(files)
}

/// Select the primary output file from a sorted list.
pub(crate) fn find_primary_output(files: &[std::path::PathBuf]) -> Option<&std::path::PathBuf> {
    for f in files {
        if let Some(name) = f.file_name().and_then(|n| n.to_str()) {
            if name.starts_with("result") {
                return Some(f);
            }
        }
    }
    files.first()
}

/// Infer content type from file extension.
pub(crate) fn infer_output_content_type(path: &std::path::Path) -> String {
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
pub(crate) fn content_type_to_extension(content_type: &str) -> String {
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
    use crate::registry::{RuntimeEnvironmentDef, RuntimeTransformDef, VersionedName};
    use ozzy_core::toml_spec::{EdgeDef, EndpointDef, EndpointParamDef, NodeDef};
    use ozzy_types::ports::{TypedPort, TypedPortSet};
    use ozzy_types::syntax::TypeRefExpr;

    fn make_test_endpoint() -> EndpointDef {
        let mut nodes = HashMap::new();
        let mut inputs = TypedPortSet::default();
        inputs.insert(
            "raw",
            TypedPort::new(TypeRefExpr::new("std/Input", Some("1".to_string()))),
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
            description: Some("test endpoint".to_string()),
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

    fn make_runtime_transform(source: Option<&str>, command: Option<&str>) -> RuntimeTransformDef {
        RuntimeTransformDef {
            versioned_name: VersionedName::new("clean_wp", "1"),
            row_id: Uuid::nil(),
            source: source.map(str::to_owned),
            command: command.map(str::to_owned),
            environment: RuntimeEnvironmentDef {
                versioned_name: VersionedName::new("python_sci", "1"),
                row_id: Uuid::nil(),
                definition: ozzy_core::toml_spec::PublishedEnvironmentDef::BaseLockfile {
                    base: "python".to_string(),
                    installer: ozzy_core::toml_spec::BaseLockfileInstaller::PipRequirements,
                    lockfile_content: "polars==1.0\n".to_string(),
                },
            },
            description: None,
            inputs: TypedPortSet::default(),
            outputs: {
                let mut ports = TypedPortSet::default();
                ports.insert(
                    "result",
                    TypedPort::new(TypeRefExpr::new("std/Result", Some("1".to_string()))),
                );
                ports
            },
            params_schema: serde_json::json!({}),
            network: false,
            secrets: Vec::new(),
        }
    }

    #[test]
    fn test_build_execution_order() {
        let endpoint = make_test_endpoint();
        let order = build_execution_order(&endpoint).unwrap();
        assert_eq!(order.len(), 2);
        let pos1 = order.iter().position(|n| n == "step1").unwrap();
        let pos2 = order.iter().position(|n| n == "step2").unwrap();
        assert!(pos1 < pos2);
    }

    #[test]
    fn test_find_terminal_node() {
        let endpoint = make_test_endpoint();
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
            },
        );
        let endpoint = EndpointDef {
            description: None,
            params: HashMap::new(),
            inputs: TypedPortSet::default(),
            nodes,
            edges: vec![EdgeDef {
                from: "input:input".to_string(),
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
        assert_eq!(map.get("step1").unwrap()[0], ("data", "input:raw"));

        assert_eq!(map.get("step2").unwrap().len(), 1);
        assert_eq!(map.get("step2").unwrap()[0], ("input", "step1"));
    }

    #[test]
    fn test_endpoint_requires_source_code_when_any_node_uses_source() {
        let endpoint = make_test_endpoint();
        let transforms = HashMap::from([
            (
                "qc".to_string(),
                make_runtime_transform(Some("transforms/qc.py:run"), None),
            ),
            (
                "analyze".to_string(),
                make_runtime_transform(None, Some("python -m analyze")),
            ),
        ]);

        assert!(endpoint_requires_source_code(&endpoint, &transforms).unwrap());
    }

    #[test]
    fn test_compute_source_hash_requires_extracted_source_for_source_transforms() {
        let transform = make_runtime_transform(Some("transforms/qc.py:run"), None);
        let node = NodeDef {
            transform: "qc".to_string(),
            params: HashMap::new(),
        };

        let err = compute_source_hash(&transform, &node, &None).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("requires extracted source code"));
    }

    #[test]
    fn test_validate_node_input_bindings_rejects_missing_and_unexpected_ports() {
        let mut transform = make_runtime_transform(None, Some("python -m analyze"));
        transform.inputs.insert(
            "raw",
            TypedPort::new(TypeRefExpr::new("std/Input", Some("1".to_string()))),
        );
        transform.inputs.insert(
            "meta",
            TypedPort::new(TypeRefExpr::new("std/Meta", Some("1".to_string()))),
        );

        let err = validate_node_input_bindings("clean", &transform, ["raw", "extra"].into_iter())
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("missing inputs [\"meta\"]"));
        assert!(msg.contains("unexpected inputs [\"extra\"]"));
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
            inputs: TypedPortSet::default(),
            nodes: HashMap::new(),
            edges: vec![],
        };

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
            inputs: TypedPortSet::default(),
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
            inputs: TypedPortSet::default(),
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
            inputs: TypedPortSet::default(),
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
            inputs: TypedPortSet::default(),
            edges: vec![],
        };

        let endpoint_params = serde_json::json!({
            "user_threshold": 12.5,
            "other_param": "csv",
        });

        let resolved = resolve_node_params("mynode", &node, &endpoint, &endpoint_params);
        assert_eq!(resolved.get("static_key").unwrap(), "static_val");
        assert_eq!(resolved.get("threshold").unwrap(), 12.5);
        assert!(resolved.get("other_param").is_none());
        assert!(resolved.get("value").is_none());
    }

    #[test]
    fn test_coerce_param_value_float() {
        let v = serde_json::json!("12.5");
        let c = coerce_param_value(&v, "float").expect("float coercion");
        assert_eq!(c, serde_json::json!(12.5));
    }

    #[test]
    fn test_coerce_param_value_int() {
        let v = serde_json::json!("42");
        let c = coerce_param_value(&v, "int").expect("int coercion");
        assert_eq!(c, serde_json::json!(42));
    }

    #[test]
    fn test_coerce_param_value_bool() {
        assert_eq!(
            coerce_param_value(&serde_json::json!("true"), "bool").expect("bool coercion"),
            serde_json::json!(true)
        );
        assert_eq!(
            coerce_param_value(&serde_json::json!("false"), "bool").expect("bool coercion"),
            serde_json::json!(false)
        );
    }

    #[test]
    fn test_coerce_param_value_string_passthrough() {
        let v = serde_json::json!("hello");
        let c = coerce_param_value(&v, "string").expect("string passthrough");
        assert_eq!(c, serde_json::json!("hello"));
    }

    #[test]
    fn test_coerce_param_value_already_typed() {
        let v = serde_json::json!(12.5);
        let c = coerce_param_value(&v, "float").expect("typed passthrough");
        assert_eq!(c, serde_json::json!(12.5));
    }

    #[test]
    fn test_validate_rejects_invalid_float() {
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
            inputs: TypedPortSet::default(),
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
            inputs: TypedPortSet::default(),
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
            inputs: TypedPortSet::default(),
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
                default: Some(serde_json::json!(200.0)),
                binds: "node.threshold".to_string(),
                min: None,
                max: Some(100.0),
                enum_values: None,
            },
        );
        let endpoint = EndpointDef {
            description: None,
            params,
            inputs: TypedPortSet::default(),
            nodes: HashMap::new(),
            edges: vec![],
        };
        let err = validate_and_resolve_params(&endpoint, &HashMap::new());
        assert!(
            err.is_err(),
            "Default value exceeding max should be rejected"
        );
    }
}
