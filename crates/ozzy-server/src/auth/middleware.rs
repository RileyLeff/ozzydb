//! Authentication middleware for extracting user from requests.

use axum::{
    Json,
    extract::FromRequestParts,
    http::{StatusCode, request::Parts},
    response::{IntoResponse, Response},
};
use chrono::Utc;

use crate::AppState;
use crate::db::{Database, User};
use ozzy_core::hash::blake3_hash;
use ozzy_core::registry::protocol::ApiError;

/// Authenticated user extracted from request (any valid token).
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user: User,
    pub scopes: Vec<String>,
}

/// Authenticated user with write scope required.
#[derive(Debug, Clone)]
pub struct WriteAuthUser {
    pub user: User,
    pub scopes: Vec<String>,
}

/// Optional authenticated user (for public endpoints).
#[derive(Debug, Clone)]
pub struct MaybeAuthUser {
    pub user: Option<User>,
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ScopeAction {
    Read,
    Write,
    Admin,
    Owner,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedScope {
    action: ScopeAction,
    owner: Option<String>,
    project: Option<String>,
}

fn parse_scope_action(raw: &str) -> Option<ScopeAction> {
    match raw {
        "read" => Some(ScopeAction::Read),
        "write" => Some(ScopeAction::Write),
        "admin" => Some(ScopeAction::Admin),
        "owner" => Some(ScopeAction::Owner),
        _ => None,
    }
}

fn parse_scope(raw_scope: &str) -> Option<ParsedScope> {
    let (action_raw, target) = match raw_scope.split_once(':') {
        Some((a, t)) => (a, Some(t)),
        None => (raw_scope, None),
    };

    let action = parse_scope_action(action_raw)?;

    if let Some(target) = target {
        let mut parts = target.split('/');
        let owner = parts.next()?.trim();
        let project = parts.next()?.trim();
        if owner.is_empty() || project.is_empty() || parts.next().is_some() {
            return None;
        }
        Some(ParsedScope {
            action,
            owner: Some(owner.to_string()),
            project: Some(project.to_string()),
        })
    } else {
        Some(ParsedScope {
            action,
            owner: None,
            project: None,
        })
    }
}

fn action_implies(have: ScopeAction, need: ScopeAction) -> bool {
    have >= need
}

fn scope_matches_project(parsed: &ParsedScope, owner: &str, project: &str) -> bool {
    match (&parsed.owner, &parsed.project) {
        (None, None) => true,
        (Some(scope_owner), Some(scope_project)) => {
            scope_owner == owner && scope_project == project
        }
        _ => false,
    }
}

pub fn has_any_scope(scopes: &[String], need: ScopeAction) -> bool {
    scopes.iter().any(|scope| {
        parse_scope(scope)
            .map(|parsed| action_implies(parsed.action, need))
            .unwrap_or(false)
    })
}

pub fn has_project_scope(scopes: &[String], need: ScopeAction, owner: &str, project: &str) -> bool {
    scopes.iter().any(|scope| {
        parse_scope(scope)
            .map(|parsed| {
                action_implies(parsed.action, need)
                    && scope_matches_project(&parsed, owner, project)
            })
            .unwrap_or(false)
    })
}

pub fn can_delegate_scopes(granter_scopes: &[String], requested_scopes: &[String]) -> bool {
    requested_scopes.iter().all(|requested| {
        let Some(requested_parsed) = parse_scope(requested) else {
            return false;
        };

        granter_scopes.iter().any(|granter| {
            let Some(granter_parsed) = parse_scope(granter) else {
                return false;
            };

            if !action_implies(granter_parsed.action, requested_parsed.action) {
                return false;
            }

            match (&granter_parsed.owner, &granter_parsed.project) {
                (None, None) => true,
                (Some(owner), Some(project)) => {
                    requested_parsed.owner.as_deref() == Some(owner.as_str())
                        && requested_parsed.project.as_deref() == Some(project.as_str())
                }
                _ => false,
            }
        })
    })
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AuthError;

    fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> impl std::future::Future<Output = Result<Self, Self::Rejection>> + Send {
        async move {
            let token = extract_token(parts)?;
            let (user, scopes) = validate_token_with_scopes(&state.db, &token).await?;
            if !has_any_scope(&scopes, ScopeAction::Read) {
                return Err(AuthError::InsufficientScope);
            }
            Ok(AuthUser { user, scopes })
        }
    }
}

impl FromRequestParts<AppState> for WriteAuthUser {
    type Rejection = AuthError;

    fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> impl std::future::Future<Output = Result<Self, Self::Rejection>> + Send {
        async move {
            let token = extract_token(parts)?;
            let (user, scopes) = validate_token_with_scopes(&state.db, &token).await?;
            if !has_any_scope(&scopes, ScopeAction::Write) {
                return Err(AuthError::InsufficientScope);
            }
            Ok(WriteAuthUser { user, scopes })
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
                Ok(token) => match validate_token_with_scopes(&state.db, &token).await {
                    Ok((user, scopes)) if has_any_scope(&scopes, ScopeAction::Read) => {
                        Ok(MaybeAuthUser {
                            user: Some(user),
                            scopes,
                        })
                    }
                    Err(_) => Ok(MaybeAuthUser {
                        user: None,
                        scopes: vec![],
                    }),
                    _ => Ok(MaybeAuthUser {
                        user: None,
                        scopes: vec![],
                    }),
                },
                Err(_) => Ok(MaybeAuthUser {
                    user: None,
                    scopes: vec![],
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

async fn validate_token_with_scopes(
    db: &Database,
    token: &str,
) -> Result<(User, Vec<String>), AuthError> {
    // Hash the token to look it up
    let token_hash = blake3_hash(token.as_bytes());

    // Look up the token
    let api_token = db
        .get_token_by_hash(&token_hash)
        .await
        .map_err(|_| AuthError::ServerError)?
        .ok_or(AuthError::InvalidToken)?;

    // Check expiration
    if let Some(expires_at) = api_token.expires_at {
        if expires_at < Utc::now() {
            return Err(AuthError::TokenExpired);
        }
    }

    // Update last_used_at only if it's been more than 5 minutes since last update.
    // This reduces DB writes while still tracking token usage accurately enough.
    let should_touch = match api_token.last_used_at {
        Some(last_used) => {
            let elapsed = Utc::now().signed_duration_since(last_used);
            elapsed.num_seconds() > TOKEN_TOUCH_INTERVAL_SECS
        }
        None => true, // Never used before, touch it
    };

    if should_touch {
        let _ = db.touch_token(api_token.id).await;
    }

    let scopes = api_token.scopes.clone();

    // Get the user
    let user = db
        .get_user_by_id(api_token.user_id)
        .await
        .map_err(|_| AuthError::ServerError)?
        .ok_or(AuthError::InvalidToken)?;

    Ok((user, scopes))
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

        (status, Json(ApiError::new(error, message))).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::{ScopeAction, can_delegate_scopes, has_any_scope, has_project_scope};

    #[test]
    fn read_scope_accepts_read_and_write() {
        assert!(has_any_scope(&["read".to_string()], ScopeAction::Read));
        assert!(has_any_scope(&["write".to_string()], ScopeAction::Read));
    }

    #[test]
    fn read_scope_rejects_missing_scope() {
        assert!(!has_any_scope(&[], ScopeAction::Read));
        assert!(has_any_scope(&["admin".to_string()], ScopeAction::Read));
    }

    #[test]
    fn project_scope_matches_exact_target() {
        let scopes = vec!["read:alice/sapflux".to_string()];
        assert!(has_project_scope(
            &scopes,
            ScopeAction::Read,
            "alice",
            "sapflux"
        ));
        assert!(!has_project_scope(
            &scopes,
            ScopeAction::Read,
            "alice",
            "other"
        ));
    }

    #[test]
    fn delegation_prevents_scope_escalation() {
        let granter = vec!["read:alice/sapflux".to_string()];
        assert!(can_delegate_scopes(
            &granter,
            &["read:alice/sapflux".to_string()]
        ));
        assert!(!can_delegate_scopes(
            &granter,
            &["write:alice/sapflux".to_string()]
        ));
        assert!(!can_delegate_scopes(
            &granter,
            &["read:alice/other".to_string()]
        ));
    }
}
