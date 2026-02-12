//! Project inspection API.
//!
//! `GET /v1/projects/{owner}` — list user's projects
//! `GET /v1/projects/{owner}/{project}` — project detail

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use serde::Serialize;

use super::access::enforce_read_access;
use super::auth::ApiError;
use crate::AppState;
use crate::auth::middleware::MaybeAuthUser;

/// Build the projects router.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/{owner}", get(list_projects))
        .route("/{owner}/{slug}", get(get_project))
}

/// Project summary in list responses.
#[derive(Debug, Serialize)]
struct ProjectSummary {
    owner: String,
    slug: String,
    description: Option<String>,
    visibility: String,
    created_at: String,
    updated_at: String,
}

/// Detailed project response.
#[derive(Debug, Serialize)]
struct ProjectDetail {
    owner: String,
    slug: String,
    description: Option<String>,
    visibility: String,
    created_at: String,
    updated_at: String,
    commit_count: i64,
    refs: Vec<RefInfo>,
    collaborators: Vec<CollaboratorInfo>,
}

#[derive(Debug, Serialize)]
struct RefInfo {
    name: String,
    ref_type: String,
    commit_sha: Option<String>,
}

#[derive(Debug, Serialize)]
struct CollaboratorInfo {
    username: String,
    role: String,
}

/// List a user's projects.
///
/// Returns public projects for unauthenticated requests,
/// plus private projects the authenticated user has access to.
async fn list_projects(
    State(state): State<AppState>,
    Path(owner): Path<String>,
    auth: MaybeAuthUser,
) -> Result<Json<Vec<ProjectSummary>>, ApiError> {
    let user = state
        .db
        .get_user_by_username(&owner)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("User '{}' not found", owner)))?;

    let projects = state.db.list_user_projects(user.id).await?;

    let mut visible = Vec::new();
    for project in projects {
        if project.visibility == "public" {
            visible.push(project_to_summary(&owner, &project));
        } else if let Some(ref auth_user) = auth.user {
            // Check token scope — project-scoped tokens must not leak other projects
            let scope = auth.scope.as_deref().unwrap_or("");
            if !crate::auth::middleware::scope_grants_project_access(scope, &owner, &project.slug) {
                continue;
            }

            // Show private projects only if the auth user has access
            if auth_user.id == project.owner_id
                || state
                    .db
                    .get_project_collaborator(project.id, auth_user.id)
                    .await?
                    .is_some()
            {
                visible.push(project_to_summary(&owner, &project));
            }
        }
    }

    Ok(Json(visible))
}

/// Get project detail.
async fn get_project(
    State(state): State<AppState>,
    Path((owner, slug)): Path<(String, String)>,
    auth: MaybeAuthUser,
) -> Result<Json<ProjectDetail>, ApiError> {
    let project =
        state.db.get_project(&owner, &slug).await?.ok_or_else(|| {
            ApiError::not_found(format!("Project '{}/{}' not found", owner, slug))
        })?;

    enforce_read_access(&state, &project, &owner, &slug, &auth).await?;

    // Get commit count
    let commit_count = state.db.count_commits(project.id).await?;

    // Get refs
    let refs = state.db.list_refs(project.id).await?;
    let mut ref_infos = Vec::new();
    for r in &refs {
        let commit = state.db.get_commit_by_id(r.commit_id).await?;
        ref_infos.push(RefInfo {
            name: r.ref_name.clone(),
            ref_type: r.ref_type.clone(),
            commit_sha: commit.map(|c| c.git_commit_sha),
        });
    }

    // Get collaborators
    let collaborators = state.db.list_project_collaborators(project.id).await?;
    let collab_infos: Vec<CollaboratorInfo> = collaborators
        .iter()
        .map(|c| CollaboratorInfo {
            username: c.username.clone(),
            role: c.role.clone(),
        })
        .collect();

    Ok(Json(ProjectDetail {
        owner: owner.clone(),
        slug: slug.clone(),
        description: project.description,
        visibility: project.visibility,
        created_at: project.created_at.to_rfc3339(),
        updated_at: project.updated_at.to_rfc3339(),
        commit_count,
        refs: ref_infos,
        collaborators: collab_infos,
    }))
}

fn project_to_summary(owner: &str, project: &crate::db::Project) -> ProjectSummary {
    ProjectSummary {
        owner: owner.to_string(),
        slug: project.slug.clone(),
        description: project.description.clone(),
        visibility: project.visibility.clone(),
        created_at: project.created_at.to_rfc3339(),
        updated_at: project.updated_at.to_rfc3339(),
    }
}
