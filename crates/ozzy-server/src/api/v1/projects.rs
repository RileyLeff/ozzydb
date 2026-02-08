//! Project management API endpoints.

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::header,
    response::IntoResponse,
    routing::{delete, get, post},
};
use ozzy_core::project::{Endpoint, SourceType};
use ozzy_core::registry::protocol::{
    AddCollaboratorRequest, CollaboratorInfo, CommitInfo, CreateProjectRequest, ListRefsResponse,
    ProjectInfo, RefInfo,
};
use serde::Deserialize;
use std::collections::HashMap;

use super::access::{enforce_read_access, user_has_project_permission};
use super::auth::ApiError;
use crate::{
    AppState,
    auth::middleware::{AuthUser, MaybeAuthUser, ScopeAction, WriteAuthUser, has_project_scope},
};

/// Pagination query parameters.
#[derive(Debug, Deserialize)]
struct PaginationParams {
    #[serde(default = "default_limit")]
    limit: i64,
    #[serde(default)]
    offset: i64,
}

impl PaginationParams {
    /// Clamp offset to non-negative and limit to a reasonable range.
    fn sanitized(&self) -> (i64, i64) {
        let limit = self.limit.clamp(1, 100);
        let offset = self.offset.max(0);
        (limit, offset)
    }
}

fn default_limit() -> i64 {
    50
}

#[derive(Debug, Deserialize)]
struct RefParams {
    #[serde(rename = "ref")]
    ref_name: Option<String>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/projects", get(list_projects))
        .route("/projects", post(create_project))
        .route("/{owner}/{project}", get(get_project))
        .route("/{owner}/{project}/commits", get(list_commits))
        .route("/{owner}/{project}/dag", get(get_dag))
        .route("/{owner}/{project}/dag.svg", get(get_dag_svg))
        .route("/{owner}/{project}/endpoints", get(list_endpoints_at_ref))
        .route("/{owner}/{project}/transforms", get(list_transforms_at_ref))
        .route(
            "/{owner}/{project}/schemas/{name}",
            get(get_schema_definition),
        )
        .route(
            "/{owner}/{project}/transforms/{name}/source",
            get(get_transform_source),
        )
        .route("/{owner}/{project}/refs", get(list_refs))
        .route("/{owner}/{project}/collaborators", get(list_collaborators))
        .route("/{owner}/{project}/collaborators", post(add_collaborator))
        .route(
            "/{owner}/{project}/collaborators/{username}",
            delete(remove_collaborator),
        )
}

/// List projects owned by the authenticated user.
async fn list_projects(
    AuthUser { user, .. }: AuthUser,
    State(state): State<AppState>,
    Query(pagination): Query<PaginationParams>,
) -> Result<Json<Vec<ProjectInfo>>, ApiError> {
    let projects = state
        .db
        .list_user_projects_paginated(user.id, pagination.sanitized().0, pagination.sanitized().1)
        .await?;

    let infos: Vec<ProjectInfo> = projects
        .into_iter()
        .map(|p| ProjectInfo {
            id: p.id,
            owner: user.username.clone(),
            slug: p.slug,
            description: p.description,
            visibility: p.visibility,
            default_branch: p.default_branch,
            created_at: p.created_at,
            updated_at: p.updated_at,
        })
        .collect();

    Ok(Json(infos))
}

/// Validate a project slug format.
fn validate_slug(slug: &str) -> Result<(), ApiError> {
    if slug.is_empty() || slug.len() > 100 {
        return Err(ApiError::BadRequest(
            "Project slug must be 1-100 characters".to_string(),
        ));
    }
    if !slug
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    {
        return Err(ApiError::BadRequest(
            "Project slug must contain only lowercase letters, digits, hyphens, and underscores"
                .to_string(),
        ));
    }
    if slug.starts_with('-') || slug.ends_with('-') {
        return Err(ApiError::BadRequest(
            "Project slug cannot start or end with a hyphen".to_string(),
        ));
    }
    Ok(())
}

/// Create a new project.
async fn create_project(
    WriteAuthUser { user, scopes }: WriteAuthUser,
    State(state): State<AppState>,
    Json(req): Json<CreateProjectRequest>,
) -> Result<Json<ProjectInfo>, ApiError> {
    let visibility = req.visibility.as_deref().unwrap_or("private");
    if visibility == "org" {
        return Err(ApiError::bad_request(
            "visibility='org' is not supported yet",
        ));
    }
    if !matches!(visibility, "private" | "public") {
        return Err(ApiError::bad_request(
            "visibility must be one of: private, public",
        ));
    }

    validate_slug(&req.slug)?;
    if !has_project_scope(&scopes, ScopeAction::Write, &user.username, &req.slug) {
        return Err(ApiError::forbidden(
            "Token lacks write scope for this project slug",
        ));
    }

    let project = state
        .db
        .create_project(user.id, &req.slug, req.description.as_deref(), visibility)
        .await?;

    Ok(Json(ProjectInfo {
        id: project.id,
        owner: user.username,
        slug: project.slug,
        description: project.description,
        visibility: project.visibility,
        default_branch: project.default_branch,
        created_at: project.created_at,
        updated_at: project.updated_at,
    }))
}

/// Get project info.
async fn get_project(
    auth: MaybeAuthUser,
    State(state): State<AppState>,
    Path((owner, project_slug)): Path<(String, String)>,
) -> Result<Json<ProjectInfo>, ApiError> {
    let project = state
        .db
        .get_project(&owner, &project_slug)
        .await?
        .ok_or_else(|| ApiError::NotFound("Project not found".to_string()))?;

    enforce_read_access(&state, &project, &owner, &project_slug, &auth).await?;

    Ok(Json(ProjectInfo {
        id: project.id,
        owner,
        slug: project.slug,
        description: project.description,
        visibility: project.visibility,
        default_branch: project.default_branch,
        created_at: project.created_at,
        updated_at: project.updated_at,
    }))
}

/// List commit history for a project.
async fn list_commits(
    auth: MaybeAuthUser,
    State(state): State<AppState>,
    Path((owner, project_slug)): Path<(String, String)>,
    Query(pagination): Query<PaginationParams>,
) -> Result<Json<Vec<CommitInfo>>, ApiError> {
    let project = state
        .db
        .get_project(&owner, &project_slug)
        .await?
        .ok_or_else(|| ApiError::NotFound("Project not found".to_string()))?;

    enforce_read_access(&state, &project, &owner, &project_slug, &auth).await?;

    let commits = state
        .db
        .list_commits_paginated(
            project.id,
            pagination.sanitized().0,
            pagination.sanitized().1,
        )
        .await?;

    let infos = commits
        .into_iter()
        .map(|c| CommitInfo {
            hash: c.hash,
            parent_hashes: c.parent_hashes,
            author: c.author_name,
            message: c.message,
            created_at: c.created_at,
        })
        .collect();

    Ok(Json(infos))
}

async fn resolve_project_ref_commit(
    state: &AppState,
    auth: &MaybeAuthUser,
    owner: &str,
    project_slug: &str,
    ref_name: Option<&str>,
) -> Result<(crate::db::Project, crate::db::DbCommit, String), ApiError> {
    let project = state
        .db
        .get_project(owner, project_slug)
        .await?
        .ok_or_else(|| ApiError::NotFound("Project not found".to_string()))?;

    enforce_read_access(state, &project, owner, project_slug, auth).await?;

    let resolved_ref = ref_name.unwrap_or("main").to_string();
    let db_ref = state
        .db
        .get_ref_by_name(project.id, &resolved_ref)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Ref not found: {}", resolved_ref)))?;

    let commit = state
        .db
        .get_commit_by_id(db_ref.commit_id)
        .await?
        .ok_or_else(|| ApiError::NotFound("Commit not found".to_string()))?;

    Ok((project, commit, resolved_ref))
}

/// Get DAG metadata at a ref.
async fn get_dag(
    auth: MaybeAuthUser,
    State(state): State<AppState>,
    Path((owner, project_slug)): Path<(String, String)>,
    Query(ref_params): Query<RefParams>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let (_, commit, resolved_ref) = resolve_project_ref_commit(
        &state,
        &auth,
        &owner,
        &project_slug,
        ref_params.ref_name.as_deref(),
    )
    .await?;

    let data_sources = state.db.get_data_sources(commit.id).await?;
    let transforms = state.db.get_transforms(commit.id).await?;
    let endpoints = state.db.get_endpoints(commit.id).await?;

    let data_json: Vec<serde_json::Value> = data_sources
        .iter()
        .map(|ds| {
            serde_json::json!({
                "name": ds.name,
                "hash": ds.content_hash,
                "schema_hash": ds.schema_hash,
                "schema": ds.schema_json,
                "row_count": ds.row_count,
                "byte_size": ds.byte_size,
            })
        })
        .collect();
    let transform_json: Vec<serde_json::Value> = transforms
        .iter()
        .map(|t| {
            serde_json::json!({
                "name": t.name,
                "hash": t.content_hash,
                "runtime": t.runtime,
                "source_path": t.source_storage_key,
                "function_name": t.function_name,
                "lockfile_hash": t.lockfile_hash,
                "params_schema": t.params_schema,
                "input_schema": t.input_schema,
                "output_schema": t.output_schema,
                "reproducible": t.reproducible,
            })
        })
        .collect();
    let endpoint_json: Vec<serde_json::Value> = endpoints
        .iter()
        .map(|e| {
            serde_json::json!({
                "name": e.name,
                "description": e.description,
                "definition": e.definition,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "owner": owner,
        "project": project_slug,
        "ref_name": resolved_ref,
        "commit_hash": commit.hash,
        "data_sources": data_json,
        "transforms": transform_json,
        "endpoints": endpoint_json,
    })))
}

/// Render DAG as SVG.
async fn get_dag_svg(
    auth: MaybeAuthUser,
    State(state): State<AppState>,
    Path((owner, project_slug)): Path<(String, String)>,
    Query(ref_params): Query<RefParams>,
) -> Result<impl IntoResponse, ApiError> {
    let (_, commit, _) = resolve_project_ref_commit(
        &state,
        &auth,
        &owner,
        &project_slug,
        ref_params.ref_name.as_deref(),
    )
    .await?;

    let data_sources = state.db.get_data_sources(commit.id).await?;
    let transforms = state.db.get_transforms(commit.id).await?;
    let endpoints = state.db.get_endpoints(commit.id).await?;

    let endpoint_defs: Vec<Endpoint> = endpoints
        .iter()
        .map(|e| {
            serde_json::from_value::<Endpoint>(e.definition.clone()).unwrap_or_else(|err| {
                tracing::warn!("Failed to parse endpoint '{}': {}", e.name, err);
                Endpoint {
                    name: e.name.clone(),
                    nodes: Vec::new(),
                    edges: Vec::new(),
                    description: e.description.clone(),
                }
            })
        })
        .collect();

    let svg = render_dag_svg(&data_sources, &transforms, &endpoint_defs);
    Ok((
        [(header::CONTENT_TYPE, "image/svg+xml; charset=utf-8")],
        svg,
    ))
}

/// List endpoints at a ref.
async fn list_endpoints_at_ref(
    auth: MaybeAuthUser,
    State(state): State<AppState>,
    Path((owner, project_slug)): Path<(String, String)>,
    Query(ref_params): Query<RefParams>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let (_, commit, resolved_ref) = resolve_project_ref_commit(
        &state,
        &auth,
        &owner,
        &project_slug,
        ref_params.ref_name.as_deref(),
    )
    .await?;

    let mut endpoints = state.db.get_endpoints(commit.id).await?;
    endpoints.sort_by(|a, b| a.name.cmp(&b.name));

    let items: Vec<serde_json::Value> = endpoints
        .into_iter()
        .map(|e| {
            serde_json::json!({
                "name": e.name,
                "description": e.description,
                "definition": e.definition,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "ref_name": resolved_ref,
        "commit_hash": commit.hash,
        "endpoints": items,
    })))
}

/// List transforms at a ref.
async fn list_transforms_at_ref(
    auth: MaybeAuthUser,
    State(state): State<AppState>,
    Path((owner, project_slug)): Path<(String, String)>,
    Query(ref_params): Query<RefParams>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let (_, commit, resolved_ref) = resolve_project_ref_commit(
        &state,
        &auth,
        &owner,
        &project_slug,
        ref_params.ref_name.as_deref(),
    )
    .await?;

    let mut transforms = state.db.get_transforms(commit.id).await?;
    transforms.sort_by(|a, b| a.name.cmp(&b.name));

    let items: Vec<serde_json::Value> = transforms
        .into_iter()
        .map(|t| {
            serde_json::json!({
                "name": t.name,
                "hash": t.content_hash,
                "runtime": t.runtime,
                "source_path": t.source_storage_key,
                "function_name": t.function_name,
                "lockfile_hash": t.lockfile_hash,
                "params_schema": t.params_schema,
                "input_schema": t.input_schema,
                "output_schema": t.output_schema,
                "reproducible": t.reproducible,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "ref_name": resolved_ref,
        "commit_hash": commit.hash,
        "transforms": items,
    })))
}

/// Get transform source code.
async fn get_transform_source(
    auth: MaybeAuthUser,
    State(state): State<AppState>,
    Path((owner, project_slug, name)): Path<(String, String, String)>,
    Query(ref_params): Query<RefParams>,
) -> Result<impl IntoResponse, ApiError> {
    let (_, commit, _) = resolve_project_ref_commit(
        &state,
        &auth,
        &owner,
        &project_slug,
        ref_params.ref_name.as_deref(),
    )
    .await?;

    let transforms = state.db.get_transforms(commit.id).await?;
    let transform = transforms
        .into_iter()
        .find(|t| t.name == name)
        .ok_or_else(|| ApiError::NotFound(format!("Transform '{}' not found", name)))?;

    let content = state
        .storage
        .get(&transform.content_hash, "py")
        .await
        .map_err(|e| {
            ApiError::NotFound(format!("Transform source not found in storage: {}", e))
        })?;

    let source = String::from_utf8(content.to_vec()).map_err(|_| {
        ApiError::Internal(anyhow::anyhow!("Transform source is not valid UTF-8"))
    })?;

    Ok((
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        source,
    ))
}

/// Get a schema definition by name at a ref.
async fn get_schema_definition(
    auth: MaybeAuthUser,
    State(state): State<AppState>,
    Path((owner, project_slug, name)): Path<(String, String, String)>,
    Query(ref_params): Query<RefParams>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let (_, commit, resolved_ref) = resolve_project_ref_commit(
        &state,
        &auth,
        &owner,
        &project_slug,
        ref_params.ref_name.as_deref(),
    )
    .await?;

    let data_sources = state.db.get_data_sources(commit.id).await?;
    if let Some(ds) = data_sources.into_iter().find(|ds| ds.name == name) {
        return Ok(Json(serde_json::json!({
            "name": ds.name,
            "schema_type": "data_source",
            "ref_name": resolved_ref,
            "commit_hash": commit.hash,
            "schema_hash": ds.schema_hash,
            "schema": ds.schema_json,
        })));
    }

    let transforms = state.db.get_transforms(commit.id).await?;
    if let Some(t) = transforms.into_iter().find(|t| t.name == name) {
        if t.input_schema.is_none() && t.output_schema.is_none() {
            return Err(ApiError::NotFound(format!(
                "Transform '{}' has no declared schema",
                name
            )));
        }
        return Ok(Json(serde_json::json!({
            "name": t.name,
            "schema_type": "transform",
            "ref_name": resolved_ref,
            "commit_hash": commit.hash,
            "input_schema": t.input_schema,
            "output_schema": t.output_schema,
        })));
    }

    Err(ApiError::NotFound(format!("Schema '{}' not found", name)))
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn render_dag_svg(
    data_sources: &[crate::db::DbDataSource],
    transforms: &[crate::db::DbTransform],
    endpoints: &[Endpoint],
) -> String {
    if endpoints.is_empty() {
        return "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"480\" height=\"120\"><rect width=\"100%\" height=\"100%\" fill=\"#f8fafc\"/><text x=\"24\" y=\"64\" font-family=\"monospace\" font-size=\"16\" fill=\"#0f172a\">No endpoints</text></svg>".to_string();
    }

    let data_by_name: HashMap<String, String> = data_sources
        .iter()
        .map(|ds| (ds.name.clone(), ds.name.clone()))
        .collect();
    let transform_runtime: HashMap<String, String> = transforms
        .iter()
        .map(|t| (t.name.clone(), t.runtime.clone()))
        .collect();

    let mut total_height = 40usize;
    for endpoint in endpoints {
        let rows = endpoint.edges.len().max(1);
        total_height += 60 + rows * 90 + 40;
    }

    let width = 920usize;
    let mut out = String::new();
    out.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">",
        width, total_height, width, total_height
    ));
    out.push_str("<defs>");
    out.push_str(
        "<marker id=\"arrow\" markerWidth=\"10\" markerHeight=\"7\" refX=\"9\" refY=\"3.5\" orient=\"auto\">",
    );
    out.push_str("<polygon points=\"0 0, 10 3.5, 0 7\" fill=\"#334155\" />");
    out.push_str("</marker>");
    out.push_str("</defs>");
    out.push_str("<rect width=\"100%\" height=\"100%\" fill=\"#f8fafc\" />");

    let mut y_offset = 30i32;
    for endpoint in endpoints {
        let rows = endpoint.edges.len().max(1) as i32;
        let block_height = 60 + rows * 90;
        out.push_str(&format!(
            "<rect x=\"16\" y=\"{}\" width=\"888\" height=\"{}\" rx=\"10\" fill=\"#ffffff\" stroke=\"#cbd5e1\" />",
            y_offset - 18,
            block_height + 22
        ));
        out.push_str(&format!(
            "<text x=\"30\" y=\"{}\" font-family=\"monospace\" font-size=\"16\" fill=\"#0f172a\">Endpoint: {}</text>",
            y_offset + 6,
            escape_xml(&endpoint.name)
        ));

        let mut row = 0i32;
        for edge in &endpoint.edges {
            let y = y_offset + 28 + row * 90;
            row += 1;

            let source_label = match edge.source_type {
                SourceType::DataSource => data_by_name
                    .get(&edge.source_ref)
                    .cloned()
                    .unwrap_or_else(|| edge.source_ref.clone()),
                SourceType::Node => edge.source_ref.clone(),
                SourceType::External => format!(
                    "{}/{}/{}",
                    edge.external_owner.as_deref().unwrap_or("?"),
                    edge.external_project.as_deref().unwrap_or("?"),
                    edge.external_endpoint.as_deref().unwrap_or("?")
                ),
            };

            let target_label = endpoint
                .nodes
                .iter()
                .find(|n| n.node_name == edge.target_node)
                .map(|n| {
                    let runtime_suffix = transform_runtime
                        .get(&n.transform_name)
                        .map(|r| format!(" [{}]", r))
                        .unwrap_or_default();
                    format!("{}{}", n.transform_name, runtime_suffix)
                })
                .unwrap_or_else(|| edge.target_node.clone());

            out.push_str(&format!(
                "<rect x=\"40\" y=\"{}\" width=\"260\" height=\"44\" rx=\"6\" fill=\"#e2e8f0\" stroke=\"#94a3b8\" />",
                y - 22
            ));
            out.push_str(&format!(
                "<text x=\"52\" y=\"{}\" font-family=\"monospace\" font-size=\"13\" fill=\"#0f172a\">{}</text>",
                y + 4,
                escape_xml(&source_label)
            ));

            out.push_str(&format!(
                "<rect x=\"400\" y=\"{}\" width=\"430\" height=\"44\" rx=\"6\" fill=\"#dbeafe\" stroke=\"#60a5fa\" />",
                y - 22
            ));
            out.push_str(&format!(
                "<text x=\"412\" y=\"{}\" font-family=\"monospace\" font-size=\"13\" fill=\"#0f172a\">{}</text>",
                y + 4,
                escape_xml(&target_label)
            ));

            out.push_str(&format!(
                "<line x1=\"300\" y1=\"{}\" x2=\"400\" y2=\"{}\" stroke=\"#334155\" stroke-width=\"2\" marker-end=\"url(#arrow)\" />",
                y, y
            ));
        }

        y_offset += block_height + 46;
    }

    out.push_str("</svg>");
    out
}

/// List refs (branches and tags) for a project.
async fn list_refs(
    auth: MaybeAuthUser,
    State(state): State<AppState>,
    Path((owner, project_slug)): Path<(String, String)>,
) -> Result<Json<ListRefsResponse>, ApiError> {
    let project = state
        .db
        .get_project(&owner, &project_slug)
        .await?
        .ok_or_else(|| ApiError::NotFound("Project not found".to_string()))?;

    enforce_read_access(&state, &project, &owner, &project_slug, &auth).await?;

    let refs = state.db.list_refs(project.id).await?;

    // Look up commit hashes for each ref.
    let mut branches = Vec::new();
    let mut tags = Vec::new();

    for r in refs {
        let commit_hash = if let Some(commit) = state.db.get_commit_by_id(r.commit_id).await? {
            commit.hash
        } else {
            "unknown".to_string()
        };

        let info = RefInfo {
            name: r.name,
            ref_type: r.ref_type.clone(),
            commit_hash,
            updated_at: r.updated_at.to_rfc3339(),
        };

        match r.ref_type.as_str() {
            "branch" => branches.push(info),
            "tag" => tags.push(info),
            _ => {}
        }
    }

    Ok(Json(ListRefsResponse { branches, tags }))
}

/// List collaborators for a project.
async fn list_collaborators(
    auth: MaybeAuthUser,
    State(state): State<AppState>,
    Path((owner, project_slug)): Path<(String, String)>,
) -> Result<Json<Vec<CollaboratorInfo>>, ApiError> {
    let project = state
        .db
        .get_project(&owner, &project_slug)
        .await?
        .ok_or_else(|| ApiError::NotFound("Project not found".to_string()))?;

    enforce_read_access(&state, &project, &owner, &project_slug, &auth).await?;

    let collaborators = state.db.list_project_collaborators(project.id).await?;
    let infos = collaborators
        .into_iter()
        .map(|c| CollaboratorInfo {
            username: c.username,
            permission: c.permission,
            added_at: c.created_at,
        })
        .collect();

    Ok(Json(infos))
}

/// Add or update a collaborator.
async fn add_collaborator(
    AuthUser { user, scopes }: AuthUser,
    State(state): State<AppState>,
    Path((owner, project_slug)): Path<(String, String)>,
    Json(req): Json<AddCollaboratorRequest>,
) -> Result<Json<CollaboratorInfo>, ApiError> {
    if !matches!(req.permission.as_str(), "read" | "write" | "admin") {
        return Err(ApiError::bad_request(
            "permission must be one of: read, write, admin",
        ));
    }

    let project = state
        .db
        .get_project(&owner, &project_slug)
        .await?
        .ok_or_else(|| ApiError::NotFound("Project not found".to_string()))?;

    if !has_project_scope(&scopes, ScopeAction::Admin, &owner, &project_slug) {
        return Err(ApiError::forbidden(
            "Token lacks admin scope for this project",
        ));
    }

    if !user_has_project_permission(&state, &project, user.id, ScopeAction::Admin).await? {
        return Err(ApiError::forbidden(
            "Only project admins can manage collaborators",
        ));
    }

    let target_user = state
        .db
        .get_user_by_username(&req.username)
        .await?
        .ok_or_else(|| ApiError::NotFound("Collaborator user not found".to_string()))?;

    if target_user.id == project.owner_user_id {
        return Err(ApiError::bad_request(
            "Project owner does not need collaborator permissions",
        ));
    }

    let collaborator = state
        .db
        .upsert_project_collaborator(project.id, target_user.id, &req.permission)
        .await?;

    Ok(Json(CollaboratorInfo {
        username: req.username,
        permission: collaborator.permission,
        added_at: collaborator.created_at,
    }))
}

/// Remove a collaborator.
async fn remove_collaborator(
    AuthUser { user, scopes }: AuthUser,
    State(state): State<AppState>,
    Path((owner, project_slug, username)): Path<(String, String, String)>,
) -> Result<Json<()>, ApiError> {
    let project = state
        .db
        .get_project(&owner, &project_slug)
        .await?
        .ok_or_else(|| ApiError::NotFound("Project not found".to_string()))?;

    if !has_project_scope(&scopes, ScopeAction::Admin, &owner, &project_slug) {
        return Err(ApiError::forbidden(
            "Token lacks admin scope for this project",
        ));
    }

    if !user_has_project_permission(&state, &project, user.id, ScopeAction::Admin).await? {
        return Err(ApiError::forbidden(
            "Only project admins can manage collaborators",
        ));
    }

    let target_user = state
        .db
        .get_user_by_username(&username)
        .await?
        .ok_or_else(|| ApiError::NotFound("Collaborator user not found".to_string()))?;

    if target_user.id == project.owner_user_id {
        return Err(ApiError::bad_request("Cannot remove the project owner"));
    }

    let removed = state
        .db
        .remove_project_collaborator(project.id, target_user.id)
        .await?;

    if !removed {
        return Err(ApiError::NotFound("Collaborator not found".to_string()));
    }

    Ok(Json(()))
}
