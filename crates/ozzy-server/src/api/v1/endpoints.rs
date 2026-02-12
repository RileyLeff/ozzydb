//! Endpoint inspection API (no execution — that's Phase 4).
//!
//! Reads endpoint definitions from the commit_state JSONB to provide
//! listings, detail views, and DAG visualizations.

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
use crate::db::models::{Commit, CommitState};

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
    machine: Option<String>,
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
    let (_, commit_state) =
        resolve_commit_state(&state, &owner, &slug, query.ref_name.as_deref(), &auth).await?;

    let endpoints: HashMap<String, serde_json::Value> =
        serde_json::from_value(commit_state.endpoints.clone())
            .map_err(|e| ApiError::Internal(e.into()))?;

    let mut summaries: Vec<EndpointSummary> = endpoints
        .iter()
        .map(|(name, def)| {
            let params = extract_param_summaries(def);
            let node_count = def
                .get("nodes")
                .and_then(|n| n.as_object())
                .map(|n| n.len())
                .unwrap_or(0);

            EndpointSummary {
                name: name.clone(),
                description: def
                    .get("description")
                    .and_then(|d| d.as_str())
                    .map(String::from),
                params,
                node_count,
            }
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
    let (commit, commit_state) =
        resolve_commit_state(&state, &owner, &slug, query.ref_name.as_deref(), &auth).await?;

    let endpoints: HashMap<String, serde_json::Value> =
        serde_json::from_value(commit_state.endpoints.clone())
            .map_err(|e| ApiError::Internal(e.into()))?;

    let def = endpoints
        .get(&name)
        .ok_or_else(|| ApiError::not_found(format!("Endpoint '{}' not found", name)))?;

    let params = extract_param_details(def);
    let nodes = extract_nodes(def);
    let edges = extract_edges(def);

    Ok(Json(EndpointDetail {
        name,
        description: def
            .get("description")
            .and_then(|d| d.as_str())
            .map(String::from),
        commit_sha: commit.git_commit_sha,
        params,
        nodes,
        edges,
    }))
}

/// Get endpoint DAG in the requested format.
async fn get_endpoint_dag(
    State(state): State<AppState>,
    Path((owner, slug, name)): Path<(String, String, String)>,
    Query(query): Query<DagQuery>,
    auth: MaybeAuthUser,
) -> Result<Json<DagResponse>, ApiError> {
    let (_, commit_state) =
        resolve_commit_state(&state, &owner, &slug, query.ref_name.as_deref(), &auth).await?;

    let endpoints: HashMap<String, serde_json::Value> =
        serde_json::from_value(commit_state.endpoints.clone())
            .map_err(|e| ApiError::Internal(e.into()))?;

    let def = endpoints
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

/// Resolve a project's commit state at a given ref (or latest commit).
async fn resolve_commit_state(
    state: &AppState,
    owner: &str,
    slug: &str,
    ref_name: Option<&str>,
    auth: &MaybeAuthUser,
) -> Result<(Commit, CommitState), ApiError> {
    let project =
        state.db.get_project(owner, slug).await?.ok_or_else(|| {
            ApiError::not_found(format!("Project '{}/{}' not found", owner, slug))
        })?;

    enforce_read_access(state, &project, owner, slug, auth).await?;

    // Resolve commit: by ref if specified, otherwise latest
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
        // Get latest commit
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

    Ok((commit, commit_state))
}

/// Extract parameter summaries from an endpoint JSON definition.
fn extract_param_summaries(def: &serde_json::Value) -> Vec<ParamSummary> {
    let Some(params) = def.get("params").and_then(|p| p.as_object()) else {
        return vec![];
    };
    params
        .iter()
        .map(|(name, p)| ParamSummary {
            name: name.clone(),
            type_: p
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or("string")
                .to_string(),
            description: p
                .get("description")
                .and_then(|d| d.as_str())
                .map(String::from),
            default: p.get("default").cloned(),
        })
        .collect()
}

/// Extract detailed parameter info from an endpoint JSON definition.
fn extract_param_details(def: &serde_json::Value) -> Vec<ParamDetail> {
    let Some(params) = def.get("params").and_then(|p| p.as_object()) else {
        return vec![];
    };
    params
        .iter()
        .map(|(name, p)| ParamDetail {
            name: name.clone(),
            type_: p
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or("string")
                .to_string(),
            description: p
                .get("description")
                .and_then(|d| d.as_str())
                .map(String::from),
            default: p.get("default").cloned(),
            min: p.get("min").and_then(|v| v.as_f64()),
            max: p.get("max").and_then(|v| v.as_f64()),
            enum_values: p.get("enum").and_then(|v| v.as_array()).cloned(),
            binds: p
                .get("binds")
                .and_then(|b| b.as_str())
                .unwrap_or("")
                .to_string(),
        })
        .collect()
}

/// Extract nodes from an endpoint JSON definition.
fn extract_nodes(def: &serde_json::Value) -> HashMap<String, NodeDetail> {
    let Some(nodes) = def.get("nodes").and_then(|n| n.as_object()) else {
        return HashMap::new();
    };
    nodes
        .iter()
        .map(|(name, n)| {
            let params: HashMap<String, serde_json::Value> = n
                .get("params")
                .and_then(|p| serde_json::from_value(p.clone()).ok())
                .unwrap_or_default();
            (
                name.clone(),
                NodeDetail {
                    transform: n
                        .get("transform")
                        .and_then(|t| t.as_str())
                        .unwrap_or("")
                        .to_string(),
                    params,
                    machine: n.get("machine").and_then(|m| m.as_str()).map(String::from),
                },
            )
        })
        .collect()
}

/// Extract edges from an endpoint JSON definition.
fn extract_edges(def: &serde_json::Value) -> Vec<EdgeDetail> {
    let Some(edges) = def.get("edges").and_then(|e| e.as_array()) else {
        return vec![];
    };
    edges
        .iter()
        .filter_map(|e| {
            Some(EdgeDetail {
                from: e.get("from")?.as_str()?.to_string(),
                to: e.get("to")?.as_str()?.to_string(),
            })
        })
        .collect()
}

/// Render a Mermaid graph from an endpoint definition.
fn render_mermaid_dag(def: &serde_json::Value, endpoint_name: &str) -> String {
    let mut lines = vec![format!("graph TD")];
    lines.push(format!("    subgraph {}", endpoint_name));

    // Add nodes
    if let Some(nodes) = def.get("nodes").and_then(|n| n.as_object()) {
        for (name, n) in nodes {
            let transform = n.get("transform").and_then(|t| t.as_str()).unwrap_or("?");
            lines.push(format!(
                "    {}[\"{}<br/><small>{}</small>\"]",
                name, name, transform
            ));
        }
    }

    // Add edges
    if let Some(edges) = def.get("edges").and_then(|e| e.as_array()) {
        for edge in edges {
            let from = edge.get("from").and_then(|f| f.as_str()).unwrap_or("?");
            let to = edge.get("to").and_then(|t| t.as_str()).unwrap_or("?");

            // Parse edge source: could be "data:name", "collection:name", or "node_name"
            let from_id = if from.contains(':') {
                // External source — create a node for it
                let safe_id = from.replace(':', "_").replace('/', "_");
                lines.push(format!("    {}(({}))", safe_id, from));
                safe_id
            } else {
                from.to_string()
            };

            // Parse edge target: "node.input"
            let to_node = to.split('.').next().unwrap_or(to);
            lines.push(format!("    {} --> {}", from_id, to_node));
        }
    }

    lines.push("    end".to_string());
    lines.join("\n")
}
