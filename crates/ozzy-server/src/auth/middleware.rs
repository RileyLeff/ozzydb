//! Authentication middleware for extracting user from requests.

use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::Utc;

use crate::db::{Database, User};
use crate::AppState;
use ozzy_core::hash::blake3_hash;
use ozzy_core::registry::protocol::ApiError;

/// Authenticated user extracted from request (any valid token).
#[derive(Debug, Clone)]
pub struct AuthUser(pub User);

/// Authenticated user with write scope required.
#[derive(Debug, Clone)]
pub struct WriteAuthUser(pub User);

/// Optional authenticated user (for public endpoints).
#[derive(Debug, Clone)]
pub struct MaybeAuthUser(pub Option<User>);

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AuthError;

    fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> impl std::future::Future<Output = Result<Self, Self::Rejection>> + Send {
        async move {
            let token = extract_token(parts)?;
            let (user, _scopes) = validate_token_with_scopes(&state.db, &token).await?;
            Ok(AuthUser(user))
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
            if !scopes.iter().any(|s| s == "write") {
                return Err(AuthError::InsufficientScope);
            }
            Ok(WriteAuthUser(user))
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
                    Ok((user, _scopes)) => Ok(MaybeAuthUser(Some(user))),
                    Err(_) => Ok(MaybeAuthUser(None)),
                },
                Err(_) => Ok(MaybeAuthUser(None)),
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

async fn validate_token_with_scopes(db: &Database, token: &str) -> Result<(User, Vec<String>), AuthError> {
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
