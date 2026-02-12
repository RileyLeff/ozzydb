//! Authentication middleware for extracting user from requests.
//!
//! v2 scope model: each token has a single `scope` field:
//! - "account" — full account access (all projects)
//! - "project:{owner}/{slug}" — scoped to a specific project

use axum::{
    Json,
    extract::FromRequestParts,
    http::{StatusCode, request::Parts},
    response::{IntoResponse, Response},
};
use chrono::Utc;

use crate::AppState;
use crate::api::v1::auth::ErrorBody;
use crate::db::{Database, User};
use ozzy_core::hash::blake3_hash;

/// Authenticated user extracted from request (any valid token).
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user: User,
    pub scope: String,
}

/// Authenticated user with account-level scope.
/// Rejects project-scoped tokens — used for /auth/me, /auth/token, etc.
#[derive(Debug, Clone)]
pub struct AccountAuthUser {
    pub user: User,
    pub scope: String,
}

/// Optional authenticated user (for public endpoints).
#[derive(Debug, Clone)]
pub struct MaybeAuthUser {
    pub user: Option<User>,
    pub scope: Option<String>,
}

/// Check if a scope grants access to a specific project.
pub fn scope_grants_project_access(scope: &str, owner: &str, slug: &str) -> bool {
    if scope == "account" {
        return true;
    }
    if let Some(target) = scope.strip_prefix("project:") {
        let expected = format!("{}/{}", owner, slug);
        return target == expected;
    }
    false
}

/// Check if a scope is account-level (not project-scoped).
pub fn is_account_scope(scope: &str) -> bool {
    scope == "account"
}

/// Check if the granter scope can delegate to create a token with the requested scope.
pub fn can_delegate_scope(granter: &str, requested: &str) -> bool {
    if granter == "account" {
        // Account scope can delegate anything
        return true;
    }
    if let (Some(granter_target), Some(requested_target)) =
        (granter.strip_prefix("project:"), requested.strip_prefix("project:"))
    {
        // Project scope can only delegate to the same project
        return granter_target == requested_target;
    }
    false
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AuthError;

    fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> impl std::future::Future<Output = Result<Self, Self::Rejection>> + Send {
        async move {
            let token = extract_token(parts)?;
            let (user, scope) = validate_token(&state.db, &token).await?;
            Ok(AuthUser { user, scope })
        }
    }
}

impl FromRequestParts<AppState> for AccountAuthUser {
    type Rejection = AuthError;

    fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> impl std::future::Future<Output = Result<Self, Self::Rejection>> + Send {
        async move {
            let token = extract_token(parts)?;
            let (user, scope) = validate_token(&state.db, &token).await?;
            if !is_account_scope(&scope) {
                return Err(AuthError::InsufficientScope);
            }
            Ok(AccountAuthUser { user, scope })
        }
    }
}

impl FromRequestParts<AppState> for MaybeAuthUser {
    type Rejection = AuthError;

    fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> impl std::future::Future<Output = Result<Self, Self::Rejection>> + Send {
        async move {
            match extract_token(parts) {
                Ok(token) => match validate_token(&state.db, &token).await {
                    Ok((user, scope)) => Ok(MaybeAuthUser {
                        user: Some(user),
                        scope: Some(scope),
                    }),
                    Err(_) => Ok(MaybeAuthUser {
                        user: None,
                        scope: None,
                    }),
                },
                Err(_) => Ok(MaybeAuthUser {
                    user: None,
                    scope: None,
                }),
            }
        }
    }
}

fn extract_token(parts: &Parts) -> Result<String, AuthError> {
    let auth_header = parts
        .headers
        .get("Authorization")
        .ok_or(AuthError::MissingToken)?
        .to_str()
        .map_err(|_| AuthError::InvalidToken)?;

    if let Some(token) = auth_header.strip_prefix("Bearer ") {
        Ok(token.to_string())
    } else {
        Err(AuthError::InvalidToken)
    }
}

/// Minimum interval between token touch updates (5 minutes).
const TOKEN_TOUCH_INTERVAL_SECS: i64 = 300;

async fn validate_token(
    db: &Database,
    token: &str,
) -> Result<(User, String), AuthError> {
    let token_hash = blake3_hash(token.as_bytes());

    let api_token = db
        .get_token_by_hash(&token_hash)
        .await
        .map_err(|_| AuthError::ServerError)?
        .ok_or(AuthError::InvalidToken)?;

    if let Some(expires_at) = api_token.expires_at {
        if expires_at < Utc::now() {
            return Err(AuthError::TokenExpired);
        }
    }

    let should_touch = match api_token.last_used_at {
        Some(last_used) => {
            let elapsed = Utc::now().signed_duration_since(last_used);
            elapsed.num_seconds() > TOKEN_TOUCH_INTERVAL_SECS
        }
        None => true,
    };

    if should_touch {
        let _ = db.touch_token(api_token.id).await;
    }

    let scope = api_token.scope.clone();

    let user = db
        .get_user_by_id(api_token.user_id)
        .await
        .map_err(|_| AuthError::ServerError)?
        .ok_or(AuthError::InvalidToken)?;

    Ok((user, scope))
}

/// Authentication error types.
#[derive(Debug)]
pub enum AuthError {
    MissingToken,
    InvalidToken,
    TokenExpired,
    InsufficientScope,
    ServerError,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, error, message) = match self {
            AuthError::MissingToken => (
                StatusCode::UNAUTHORIZED,
                "missing_token",
                "Authorization header required",
            ),
            AuthError::InvalidToken => (
                StatusCode::UNAUTHORIZED,
                "invalid_token",
                "Invalid or unknown token",
            ),
            AuthError::TokenExpired => (
                StatusCode::UNAUTHORIZED,
                "token_expired",
                "Token has expired",
            ),
            AuthError::InsufficientScope => (
                StatusCode::FORBIDDEN,
                "insufficient_scope",
                "Token does not have the required scope for this operation",
            ),
            AuthError::ServerError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "server_error",
                "Internal server error",
            ),
        };

        (status, Json(ErrorBody::new(error, message))).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_scope_grants_all_project_access() {
        assert!(scope_grants_project_access("account", "alice", "sapflux"));
        assert!(scope_grants_project_access("account", "bob", "anything"));
    }

    #[test]
    fn project_scope_grants_matching_project() {
        assert!(scope_grants_project_access(
            "project:alice/sapflux",
            "alice",
            "sapflux"
        ));
    }

    #[test]
    fn project_scope_rejects_different_project() {
        assert!(!scope_grants_project_access(
            "project:alice/sapflux",
            "alice",
            "other"
        ));
        assert!(!scope_grants_project_access(
            "project:alice/sapflux",
            "bob",
            "sapflux"
        ));
    }

    #[test]
    fn is_account_scope_works() {
        assert!(is_account_scope("account"));
        assert!(!is_account_scope("project:alice/sapflux"));
        assert!(!is_account_scope(""));
    }

    #[test]
    fn account_can_delegate_anything() {
        assert!(can_delegate_scope("account", "account"));
        assert!(can_delegate_scope("account", "project:alice/sapflux"));
    }

    #[test]
    fn project_scope_delegates_only_same_project() {
        assert!(can_delegate_scope(
            "project:alice/sapflux",
            "project:alice/sapflux"
        ));
        assert!(!can_delegate_scope(
            "project:alice/sapflux",
            "project:alice/other"
        ));
        assert!(!can_delegate_scope("project:alice/sapflux", "account"));
    }
}
