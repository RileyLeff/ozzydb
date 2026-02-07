//! Shared access control helpers for API endpoints.

use super::auth::ApiError;
use crate::{
    AppState,
    auth::middleware::{MaybeAuthUser, ScopeAction, has_project_scope},
    db::Project,
};

/// Check if a collaborator permission level satisfies the required action.
pub fn collaborator_allows(permission: &str, need: ScopeAction) -> bool {
    match need {
        ScopeAction::Read => matches!(permission, "read" | "write" | "admin"),
        ScopeAction::Write => matches!(permission, "write" | "admin"),
        ScopeAction::Admin => permission == "admin",
        ScopeAction::Owner => false,
    }
}

/// Check if a user has a given permission level on a project (owner or collaborator).
pub async fn user_has_project_permission(
    state: &AppState,
    project: &Project,
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

/// Enforce read access on a project: public projects are open, private require auth + scope.
pub async fn enforce_read_access(
    state: &AppState,
    project: &Project,
    owner: &str,
    project_slug: &str,
    auth: &MaybeAuthUser,
) -> Result<(), ApiError> {
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

/// Enforce write access on a project: requires auth + write scope + write permission.
pub async fn enforce_write_access(
    state: &AppState,
    project: &Project,
    owner: &str,
    project_slug: &str,
    user: &crate::db::User,
    scopes: &[String],
) -> Result<(), ApiError> {
    if !has_project_scope(scopes, ScopeAction::Write, owner, project_slug) {
        return Err(ApiError::forbidden(
            "Token lacks write scope for this project",
        ));
    }

    if user_has_project_permission(state, project, user.id, ScopeAction::Write).await? {
        Ok(())
    } else {
        Err(ApiError::forbidden(
            "You don't have write access to this project",
        ))
    }
}
