//! Authentication API endpoints.

use axum::{
    Json, Router,
    extract::State,
    routing::{get, post},
};
use ozzy_core::registry::protocol::{
    AuthResponse, CreateTokenRequest, CreateTokenResponse, DeviceCodeResponse, TokenInfo, UserInfo,
};
use serde::Deserialize;

use crate::{
    AppState,
    auth::{
        github,
        middleware::{AuthUser, WriteAuthUser, can_delegate_scopes},
        tokens,
    },
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/github/device", post(github_device))
        .route("/github/poll", post(github_poll))
        .route("/token", post(create_token))
        .route("/token", get(list_tokens))
        .route("/token/{name}", axum::routing::delete(delete_token))
        .route("/me", get(get_me))
}

/// Initiate GitHub device flow.
async fn github_device(
    State(state): State<AppState>,
) -> Result<Json<DeviceCodeResponse>, ApiError> {
    let response = github::initiate_device_flow(&state.config).await?;

    Ok(Json(DeviceCodeResponse {
        device_code: response.device_code,
        user_code: response.user_code,
        verification_uri: response.verification_uri,
        expires_in: response.expires_in,
        interval: response.interval,
    }))
}

#[derive(Deserialize)]
struct PollRequest {
    device_code: String,
}

/// Poll for device flow completion.
async fn github_poll(
    State(state): State<AppState>,
    Json(req): Json<PollRequest>,
) -> Result<Json<AuthResponse>, ApiError> {
    // Try to get the token
    let access_token = match github::poll_device_flow(&state.config, &req.device_code).await? {
        Some(token) => token,
        None => {
            return Ok(Json(AuthResponse {
                access_token: None,
                token_type: None,
                user: None,
                pending: true,
            }));
        }
    };

    // Get GitHub user info
    let gh_user = github::get_github_user(&access_token).await?;

    // Upsert user in database
    let user = state
        .db
        .upsert_user_from_github(
            gh_user.id,
            &gh_user.login,
            gh_user.email.as_deref(),
            gh_user.avatar_url.as_deref(),
        )
        .await?;

    // Generate API token for the user
    let (plaintext_token, token_hash) = tokens::generate_api_token();
    let token_prefix = &plaintext_token[..12.min(plaintext_token.len())];

    // Replace any existing cli-session token so repeated logins don't hit UNIQUE(user_id, name).
    state
        .db
        .delete_token_by_name(user.id, "cli-session")
        .await?;

    state
        .db
        .create_token(
            user.id,
            "cli-session",
            &token_hash,
            token_prefix,
            &["owner".to_string()],
            Some(chrono::Utc::now() + chrono::Duration::days(90)),
        )
        .await?;

    Ok(Json(AuthResponse {
        access_token: Some(plaintext_token),
        token_type: Some("bearer".to_string()),
        user: Some(UserInfo {
            id: user.id,
            username: user.username,
            email: user.email,
            avatar_url: user.avatar_url,
        }),
        pending: false,
    }))
}

/// Create a new API token.
async fn create_token(
    WriteAuthUser { user, scopes }: WriteAuthUser,
    State(state): State<AppState>,
    Json(req): Json<CreateTokenRequest>,
) -> Result<Json<CreateTokenResponse>, ApiError> {
    if req.scopes.is_empty() {
        return Err(ApiError::bad_request("At least one scope is required"));
    }
    if !can_delegate_scopes(&scopes, &req.scopes) {
        return Err(ApiError::forbidden(
            "Cannot grant scopes you do not already have",
        ));
    }

    let (plaintext_token, token_hash) = tokens::generate_api_token();
    let token_prefix = &plaintext_token[..12.min(plaintext_token.len())];

    let expires_at = req
        .expires_in_days
        .map(|days| chrono::Utc::now() + chrono::Duration::days(days as i64));

    let token = state
        .db
        .create_token(
            user.id,
            &req.name,
            &token_hash,
            token_prefix,
            &req.scopes,
            expires_at,
        )
        .await?;

    Ok(Json(CreateTokenResponse {
        token: plaintext_token,
        id: token.id,
        name: token.name,
        scopes: token.scopes,
        expires_at: token.expires_at,
        created_at: token.created_at,
    }))
}

/// List user's API tokens.
async fn list_tokens(
    AuthUser { user, .. }: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<TokenInfo>>, ApiError> {
    let tokens = state.db.list_user_tokens(user.id).await?;

    let token_infos: Vec<TokenInfo> = tokens
        .into_iter()
        .map(|t| TokenInfo {
            id: t.id,
            name: t.name,
            scopes: t.scopes,
            created_at: t.created_at,
            expires_at: t.expires_at,
            last_used_at: t.last_used_at,
        })
        .collect();

    Ok(Json(token_infos))
}

/// Delete an API token.
async fn delete_token(
    AuthUser { user, .. }: AuthUser,
    State(state): State<AppState>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> Result<Json<()>, ApiError> {
    state.db.delete_token_by_name(user.id, &name).await?;
    Ok(Json(()))
}

/// Get current user info.
async fn get_me(AuthUser { user, .. }: AuthUser) -> Json<UserInfo> {
    Json(UserInfo {
        id: user.id,
        username: user.username,
        email: user.email,
        avatar_url: user.avatar_url,
    })
}

/// API error type with proper HTTP status codes.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// 400 Bad Request - invalid input
    #[error("{0}")]
    BadRequest(String),
    /// 401 Unauthorized - missing or invalid auth
    #[error("{0}")]
    Unauthorized(String),
    /// 403 Forbidden - authenticated but not allowed
    #[error("{0}")]
    Forbidden(String),
    /// 404 Not Found - resource doesn't exist
    #[error("{0}")]
    NotFound(String),
    /// 409 Conflict - resource already exists
    #[error("{0}")]
    Conflict(String),
    /// 500 Internal Server Error - unexpected error
    #[error(transparent)]
    Internal(anyhow::Error),
}

impl ApiError {
    pub fn not_found(resource: impl Into<String>) -> Self {
        Self::NotFound(resource.into())
    }

    pub fn forbidden(msg: impl Into<String>) -> Self {
        Self::Forbidden(msg.into())
    }

    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::BadRequest(msg.into())
    }

    pub fn unauthorized(msg: impl Into<String>) -> Self {
        Self::Unauthorized(msg.into())
    }

    pub fn conflict(msg: impl Into<String>) -> Self {
        Self::Conflict(msg.into())
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(err: anyhow::Error) -> Self {
        if let Some(sqlx_err) = err.downcast_ref::<sqlx::Error>() {
            match sqlx_err {
                sqlx::Error::RowNotFound => return Self::not_found("Resource not found"),
                sqlx::Error::Database(db_err) => {
                    if db_err.is_unique_violation() {
                        return Self::conflict("Resource already exists");
                    }
                    if db_err.is_foreign_key_violation() {
                        return Self::bad_request("Referenced resource does not exist");
                    }
                }
                _ => {}
            }
        }
        Self::Internal(err)
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(err: serde_json::Error) -> Self {
        Self::BadRequest(format!("Invalid JSON: {}", err))
    }
}

impl From<axum::extract::multipart::MultipartError> for ApiError {
    fn from(err: axum::extract::multipart::MultipartError) -> Self {
        Self::BadRequest(format!("Multipart error: {}", err))
    }
}

impl From<std::io::Error> for ApiError {
    fn from(err: std::io::Error) -> Self {
        Self::Internal(anyhow::anyhow!("IO error: {}", err))
    }
}

impl axum::response::IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        use axum::http::StatusCode;

        let (status, message) = match self {
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            ApiError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg),
            ApiError::Forbidden(msg) => (StatusCode::FORBIDDEN, msg),
            ApiError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            ApiError::Conflict(msg) => (StatusCode::CONFLICT, msg),
            ApiError::Internal(err) => {
                // Log internal errors but don't expose details to clients
                tracing::error!("Internal error: {:?}", err);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            }
        };

        let body = serde_json::json!({
            "error": message
        });

        (status, axum::Json(body)).into_response()
    }
}
