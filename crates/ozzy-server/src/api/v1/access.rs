//! Shared access control helpers for v2 API endpoints.
//!
//! v2 scope model: tokens have a single `scope` field ("account" or "project:{owner}/{slug}").
//! Project access is determined by: owner, collaborator role, or public visibility.

use super::auth::ApiError;
use crate::{
    AppState,
    auth::middleware::{MaybeAuthUser, scope_grants_project_access},
    db::Project,
};

/// Check if a collaborator role satisfies a required access level.
pub fn role_allows(role: &str, need: &str) -> bool {
    match need {
        "read" => matches!(role, "read" | "write" | "admin"),
        "write" => matches!(role, "write" | "admin"),
        "admin" => role == "admin",
        _ => false,
    }
}

/// Check if a user has a given access level on a project (owner or collaborator).
pub async fn user_has_project_access(
    state: &AppState,
    project: &Project,
    user_id: uuid::Uuid,
    need: &str,
) -> Result<bool, ApiError> {
    // Owner has all access
    if user_id == project.owner_id {
        return Ok(true);
    }
    let collaborator = state
        .db
        .get_project_collaborator(project.id, user_id)
        .await?;
    Ok(collaborator
        .as_ref()
        .map(|c| role_allows(&c.role, need))
        .unwrap_or(false))
}

/// Enforce read access on a project: public projects are open, private require auth + scope.
pub async fn enforce_read_access(
    state: &AppState,
    project: &Project,
    owner: &str,
    slug: &str,
    auth: &MaybeAuthUser,
) -> Result<(), ApiError> {
    if project.visibility == "public" {
        return Ok(());
    }

    let user = auth
        .user
        .as_ref()
        .ok_or_else(|| ApiError::unauthorized("Authentication required for private projects"))?;

    let scope = auth.scope.as_deref().unwrap_or("");
    if !scope_grants_project_access(scope, owner, slug) {
        return Err(ApiError::forbidden(
            "Token does not have access to this project",
        ));
    }

    if user_has_project_access(state, project, user.id, "read").await? {
        Ok(())
    } else {
        Err(ApiError::forbidden("You don't have access to this project"))
    }
}

/// Enforce write access on a project: requires auth + scope + write permission.
pub async fn enforce_write_access(
    state: &AppState,
    project: &Project,
    owner: &str,
    slug: &str,
    user: &crate::db::User,
    scope: &str,
) -> Result<(), ApiError> {
    if !scope_grants_project_access(scope, owner, slug) {
        return Err(ApiError::forbidden(
            "Token does not have access to this project",
        ));
    }

    if user_has_project_access(state, project, user.id, "write").await? {
        Ok(())
    } else {
        Err(ApiError::forbidden(
            "You don't have write access to this project",
        ))
    }
}
