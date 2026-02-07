//! Project management API endpoints.

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{delete, get, post},
};
use ozzy_core::registry::protocol::{
    AddCollaboratorRequest, CollaboratorInfo, CommitInfo, CreateProjectRequest, ListRefsResponse,
    ProjectInfo, RefInfo,
};
use serde::Deserialize;

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

fn default_limit() -> i64 {
    50
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/projects", get(list_projects))
        .route("/projects", post(create_project))
        .route("/{owner}/{project}", get(get_project))
        .route("/{owner}/{project}/commits", get(list_commits))
        .route("/{owner}/{project}/refs", get(list_refs))
        .route("/{owner}/{project}/collaborators", get(list_collaborators))
        .route("/{owner}/{project}/collaborators", post(add_collaborator))
        .route(
            "/{owner}/{project}/collaborators/{username}",
            delete(remove_collaborator),
        )
}

fn collaborator_allows(permission: &str, need: ScopeAction) -> bool {
    match need {
        ScopeAction::Read => matches!(permission, "read" | "write" | "admin"),
        ScopeAction::Write => matches!(permission, "write" | "admin"),
        ScopeAction::Admin => permission == "admin",
        ScopeAction::Owner => false,
    }
}

async fn user_has_project_permission(
    state: &AppState,
    project: &crate::db::Project,
    user_id: uuid::Uuid,
    need: ScopeAction,
) -> Result<bool, ApiError> {
    if user_id == project.owner_user_id {
        return Ok(true);
    }

    let collaborator = state
        .db
        .get_project_collaborator(project.id, user_id)
        .await?;
    Ok(collaborator
        .as_ref()
        .map(|c| collaborator_allows(&c.permission, need))
        .unwrap_or(false))
}

async fn enforce_read_access(
    state: &AppState,
    project: &crate::db::Project,
    owner: &str,
    project_slug: &str,
    auth: &MaybeAuthUser,
) -> Result<(), ApiError> {
    // Public projects are readable without authentication.
    if project.visibility == "public" {
        return Ok(());
    }

    let user = auth.user.as_ref().ok_or_else(|| {
        ApiError::unauthorized("Authentication required for private/org projects")
    })?;

    if !has_project_scope(&auth.scopes, ScopeAction::Read, owner, project_slug) {
        return Err(ApiError::forbidden(
            "Token lacks read scope for this project",
        ));
    }

    if user_has_project_permission(state, project, user.id, ScopeAction::Read).await? {
        Ok(())
    } else {
        Err(ApiError::forbidden("You don't have access to this project"))
    }
}

/// List projects owned by the authenticated user.
async fn list_projects(
    AuthUser { user, .. }: AuthUser,
    State(state): State<AppState>,
    Query(pagination): Query<PaginationParams>,
) -> Result<Json<Vec<ProjectInfo>>, ApiError> {
    let projects = state
        .db
        .list_user_projects_paginated(user.id, pagination.limit, pagination.offset)
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
        .list_commits_paginated(project.id, pagination.limit, pagination.offset)
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
