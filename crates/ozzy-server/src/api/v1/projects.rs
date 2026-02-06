//! Project management API endpoints.

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use ozzy_core::registry::protocol::{
    CommitInfo, CreateProjectRequest, ListRefsResponse, ProjectInfo, RefInfo,
};
use serde::Deserialize;

use super::auth::ApiError;
use crate::{
    AppState,
    auth::middleware::{AuthUser, MaybeAuthUser},
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
}

fn enforce_project_access(
    project: &crate::db::Project,
    user: &Option<crate::db::User>,
) -> Result<(), ApiError> {
    match project.visibility.as_str() {
        "public" => Ok(()),
        "org" => match user {
            Some(u) if u.id == project.owner_user_id => Ok(()),
            Some(_) => Err(ApiError::forbidden(
                "Organization visibility is not yet supported for non-owners",
            )),
            None => Err(ApiError::unauthorized(
                "Authentication required for org-visible projects",
            )),
        },
        _ => match user {
            Some(u) if u.id == project.owner_user_id => Ok(()),
            Some(_) => Err(ApiError::forbidden("You don't have access to this project")),
            None => Err(ApiError::unauthorized(
                "Authentication required for private projects",
            )),
        },
    }
}

/// List projects owned by the authenticated user.
async fn list_projects(
    AuthUser(user): AuthUser,
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
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Json(req): Json<CreateProjectRequest>,
) -> Result<Json<ProjectInfo>, ApiError> {
    let visibility = req.visibility.as_deref().unwrap_or("private");
    if !matches!(visibility, "private" | "public" | "org") {
        return Err(ApiError::bad_request(
            "visibility must be one of: private, public, org",
        ));
    }

    validate_slug(&req.slug)?;

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

/// Get project info (respects visibility: public projects are open, private require auth + ownership).
async fn get_project(
    MaybeAuthUser(user): MaybeAuthUser,
    State(state): State<AppState>,
    Path((owner, project_slug)): Path<(String, String)>,
) -> Result<Json<ProjectInfo>, ApiError> {
    let project = state
        .db
        .get_project(&owner, &project_slug)
        .await?
        .ok_or_else(|| ApiError::NotFound("Project not found".to_string()))?;

    enforce_project_access(&project, &user)?;

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
    MaybeAuthUser(user): MaybeAuthUser,
    State(state): State<AppState>,
    Path((owner, project_slug)): Path<(String, String)>,
    Query(pagination): Query<PaginationParams>,
) -> Result<Json<Vec<CommitInfo>>, ApiError> {
    let project = state
        .db
        .get_project(&owner, &project_slug)
        .await?
        .ok_or_else(|| ApiError::NotFound("Project not found".to_string()))?;

    enforce_project_access(&project, &user)?;

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
    MaybeAuthUser(user): MaybeAuthUser,
    State(state): State<AppState>,
    Path((owner, project_slug)): Path<(String, String)>,
) -> Result<Json<ListRefsResponse>, ApiError> {
    let project = state
        .db
        .get_project(&owner, &project_slug)
        .await?
        .ok_or_else(|| ApiError::NotFound("Project not found".to_string()))?;

    enforce_project_access(&project, &user)?;

    let refs = state.db.list_refs(project.id).await?;

    // Look up commit hashes for each ref
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
