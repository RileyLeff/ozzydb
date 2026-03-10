//! Endpoint inspection API (no execution — that's Phase 4).
//!
//! Endpoint inspection reads from the published v4 project revision so the
//! server-visible meaning of a pushed commit no longer depends on commit_state.

use std::collections::HashMap;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::get,
};
use serde::{Deserialize, Serialize};

use super::access::enforce_read_access;
use super::auth::ApiError;
use crate::AppState;
use crate::auth::middleware::MaybeAuthUser;
use crate::db::models::Commit;
use crate::registry::{PublishedProjectRevision, load_published_project_revision_by_commit};

/// Build the endpoints router.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/{owner}/{slug}", get(list_endpoints))
        .route("/{owner}/{slug}/{name}", get(get_endpoint))
        .route("/{owner}/{slug}/{name}/dag", get(get_endpoint_dag))
}

#[derive(Debug, Deserialize)]
struct RefQuery {
    #[serde(rename = "ref")]
    ref_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DagQuery {
    #[serde(rename = "ref")]
    ref_name: Option<String>,
    #[serde(default = "default_dag_format")]
    format: String,
}

fn default_dag_format() -> String {
    "json".to_string()
}

/// Endpoint summary in list responses.
#[derive(Debug, Serialize)]
struct EndpointSummary {
    name: String,
    description: Option<String>,
    params: Vec<ParamSummary>,
    node_count: usize,
}

#[derive(Debug, Serialize)]
struct ParamSummary {
    name: String,
    #[serde(rename = "type")]
    type_: String,
    description: Option<String>,
    default: Option<serde_json::Value>,
}

/// Detailed endpoint response.
#[derive(Debug, Serialize)]
struct EndpointDetail {
    name: String,
    description: Option<String>,
    commit_sha: String,
    params: Vec<ParamDetail>,
    nodes: HashMap<String, NodeDetail>,
    edges: Vec<EdgeDetail>,
}

#[derive(Debug, Serialize)]
struct ParamDetail {
    name: String,
    #[serde(rename = "type")]
    type_: String,
    description: Option<String>,
    default: Option<serde_json::Value>,
    min: Option<f64>,
    max: Option<f64>,
    #[serde(rename = "enum")]
    enum_values: Option<Vec<serde_json::Value>>,
    binds: String,
}

#[derive(Debug, Serialize)]
struct NodeDetail {
    transform: String,
    params: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct EdgeDetail {
    from: String,
    to: String,
}

/// DAG response (multiple format support).
#[derive(Debug, Serialize)]
struct DagResponse {
    format: String,
    content: String,
}

/// List endpoints for a project at a given ref.
async fn list_endpoints(
    State(state): State<AppState>,
    Path((owner, slug)): Path<(String, String)>,
    Query(query): Query<RefQuery>,
    auth: MaybeAuthUser,
) -> Result<Json<Vec<EndpointSummary>>, ApiError> {
    let (_, published) =
        resolve_published_revision(&state, &owner, &slug, query.ref_name.as_deref(), &auth).await?;

    let mut summaries: Vec<EndpointSummary> = published
        .endpoints
        .iter()
        .map(|(name, def)| EndpointSummary {
            name: name.clone(),
            description: def.description.clone(),
            params: extract_param_summaries(def),
            node_count: def.nodes.len(),
        })
        .collect();

    summaries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Json(summaries))
}

/// Get endpoint detail.
async fn get_endpoint(
    State(state): State<AppState>,
    Path((owner, slug, name)): Path<(String, String, String)>,
    Query(query): Query<RefQuery>,
    auth: MaybeAuthUser,
) -> Result<Json<EndpointDetail>, ApiError> {
    let (commit, published) =
        resolve_published_revision(&state, &owner, &slug, query.ref_name.as_deref(), &auth).await?;

    let def = published
        .endpoints
        .get(&name)
        .ok_or_else(|| ApiError::not_found(format!("Endpoint '{}' not found", name)))?;

    Ok(Json(EndpointDetail {
        name,
        description: def.description.clone(),
        commit_sha: commit.git_commit_sha,
        params: extract_param_details(def),
        nodes: extract_nodes(def),
        edges: extract_edges(def),
    }))
}

/// Get endpoint DAG in the requested format.
async fn get_endpoint_dag(
    State(state): State<AppState>,
    Path((owner, slug, name)): Path<(String, String, String)>,
    Query(query): Query<DagQuery>,
    auth: MaybeAuthUser,
) -> Result<Json<DagResponse>, ApiError> {
    let (_, published) =
        resolve_published_revision(&state, &owner, &slug, query.ref_name.as_deref(), &auth).await?;

    let def = published
        .endpoints
        .get(&name)
        .ok_or_else(|| ApiError::not_found(format!("Endpoint '{}' not found", name)))?;

    let content = match query.format.as_str() {
        "json" => serde_json::to_string_pretty(def).map_err(|e| ApiError::Internal(e.into()))?,
        "mermaid" => render_mermaid_dag(def, &name),
        _ => {
            return Err(ApiError::BadRequest(format!(
                "Unsupported DAG format: '{}'. Supported: json, mermaid",
                query.format
            )));
        }
    };

    Ok(Json(DagResponse {
        format: query.format,
        content,
    }))
}

// ── Helpers ──────────────────────────────────────────────────────

/// Resolve a project's published v4 revision at a given ref (or latest commit).
async fn resolve_published_revision(
    state: &AppState,
    owner: &str,
    slug: &str,
    ref_name: Option<&str>,
    auth: &MaybeAuthUser,
) -> Result<(Commit, PublishedProjectRevision), ApiError> {
    let project =
        state.db.get_project(owner, slug).await?.ok_or_else(|| {
            ApiError::not_found(format!("Project '{}/{}' not found", owner, slug))
        })?;

    enforce_read_access(state, &project, owner, slug, auth).await?;

    let commit = if let Some(ref_name) = ref_name {
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

    Ok((commit, published))
}

/// Extract parameter summaries from an endpoint definition.
fn extract_param_summaries(def: &ozzy_core::toml_spec::EndpointDef) -> Vec<ParamSummary> {
    def.params
        .iter()
        .map(|(name, param)| ParamSummary {
            name: name.clone(),
            type_: param.type_.clone(),
            description: param.description.clone(),
            default: param.default.clone(),
        })
        .collect()
}

/// Extract detailed parameter info from an endpoint definition.
fn extract_param_details(def: &ozzy_core::toml_spec::EndpointDef) -> Vec<ParamDetail> {
    def.params
        .iter()
        .map(|(name, param)| ParamDetail {
            name: name.clone(),
            type_: param.type_.clone(),
            description: param.description.clone(),
            default: param.default.clone(),
            min: param.min,
            max: param.max,
            enum_values: param.enum_values.clone(),
            binds: param.binds.clone(),
        })
        .collect()
}

/// Extract nodes from an endpoint definition.
fn extract_nodes(def: &ozzy_core::toml_spec::EndpointDef) -> HashMap<String, NodeDetail> {
    def.nodes
        .iter()
        .map(|(name, node)| {
            (
                name.clone(),
                NodeDetail {
                    transform: node.transform.clone(),
                    params: node.params.clone(),
                },
            )
        })
        .collect()
}

/// Extract edges from an endpoint definition.
fn extract_edges(def: &ozzy_core::toml_spec::EndpointDef) -> Vec<EdgeDetail> {
    def.edges
        .iter()
        .map(|edge| EdgeDetail {
            from: edge.from.clone(),
            to: edge.to.clone(),
        })
        .collect()
}

/// Render a Mermaid graph from an endpoint definition.
fn render_mermaid_dag(def: &ozzy_core::toml_spec::EndpointDef, endpoint_name: &str) -> String {
    let mut lines = vec!["graph TD".to_string()];
    lines.push(format!("    subgraph {}", endpoint_name));

    for (name, node) in &def.nodes {
        lines.push(format!(
            "    {}[\"{}<br/><small>{}</small>\"]",
            name, name, node.transform
        ));
    }

    for edge in &def.edges {
        let from = edge.from.as_str();
        let to = edge.to.as_str();

        let from_id = if from.contains(':') {
            let safe_id = from.replace(':', "_").replace('/', "_");
            lines.push(format!("    {}(({}))", safe_id, from));
            safe_id
        } else {
            from.to_string()
        };

        let to_node = to.split('.').next().unwrap_or(to);
        lines.push(format!("    {} --> {}", from_id, to_node));
    }

    lines.push("    end".to_string());
    lines.join("\n")
}
