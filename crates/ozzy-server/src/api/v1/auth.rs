//! Authentication API endpoints.

use axum::{
    Json, Router,
    extract::State,
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    AppState,
    auth::{
        github,
        middleware::{AccountAuthUser, can_delegate_scope},
        tokens,
    },
};

// ============================================================================
// Wire types (were in ozzy_core::registry::protocol, now inline)
// ============================================================================

#[derive(Serialize)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Serialize)]
pub struct AuthResponse {
    pub access_token: Option<String>,
    pub token_type: Option<String>,
    pub user: Option<UserInfo>,
    pub pending: bool,
}

#[derive(Serialize)]
pub struct UserInfo {
    pub id: Uuid,
    pub username: String,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateTokenRequest {
    pub name: String,
    pub scope: String,              // "account" | "project:{owner}/{slug}"
    pub expires_in_days: Option<u32>,
}

#[derive(Serialize)]
pub struct CreateTokenResponse {
    pub token: String,
    pub id: Uuid,
    pub name: String,
    pub scope: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize)]
pub struct TokenInfo {
    pub id: Uuid,
    pub name: String,
    pub scope: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
}

/// Standardized JSON error body for all API error responses.
#[derive(Serialize)]
pub struct ErrorBody {
    pub error: String,
    pub message: String,
}

impl ErrorBody {
    pub fn new(error: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            message: message.into(),
        }
    }
}

// ============================================================================
// Routes
// ============================================================================

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
    /// Optional client identifier. "web" creates a separate "web-session" token
    /// so web and CLI logins don't invalidate each other.
    #[serde(default)]
    client: Option<String>,
}

/// Poll for device flow completion.
///
/// NOTE: This endpoint is unauthenticated and called in a tight loop by `ozzy auth login`.
/// If we add public-facing deployment, consider adding rate limiting (e.g. tower-governor)
/// keyed on device_code to prevent abuse.
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

    // Check registration allowlist
    if !state.config.allowed_logins.is_empty()
        && !state
            .config
            .allowed_logins
            .iter()
            .any(|a| a.eq_ignore_ascii_case(&gh_user.login))
    {
        return Err(ApiError::forbidden(
            "Registration is currently restricted. Your GitHub account is not on the allowlist.",
        ));
    }

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

    // Use different token names for web vs CLI so they don't invalidate each other
    let token_name = match req.client.as_deref() {
        Some("web") => "web-session",
        _ => "cli-session",
    };

    // Upsert the session token atomically
    let expires = chrono::Utc::now() + chrono::Duration::days(90);
    state
        .db
        .upsert_session_token(user.id, token_name, &token_hash, expires)
        .await
        .map_err(anyhow::Error::from)?;

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
    AccountAuthUser { user, scope }: AccountAuthUser,
    State(state): State<AppState>,
    Json(req): Json<CreateTokenRequest>,
) -> Result<Json<CreateTokenResponse>, ApiError> {
    // Validate scope format
    if req.scope != "account" && !req.scope.starts_with("project:") {
        return Err(ApiError::bad_request(
            "Scope must be \"account\" or \"project:{owner}/{slug}\"",
        ));
    }
    if !can_delegate_scope(&scope, &req.scope) {
        return Err(ApiError::forbidden(
            "Cannot grant a scope you do not already have",
        ));
    }

    let (plaintext_token, token_hash) = tokens::generate_api_token();

    // Resolve project_id for project-scoped tokens
    let project_id = if let Some(target) = req.scope.strip_prefix("project:") {
        if let Some((owner, slug)) = target.split_once('/') {
            let project = state
                .db
                .get_project(owner, slug)
                .await?
                .ok_or_else(|| ApiError::not_found(format!("Project {}", target)))?;
            Some(project.id)
        } else {
            return Err(ApiError::bad_request("Project scope must be \"project:{owner}/{slug}\""));
        }
    } else {
        None
    };

    let expires_at = req
        .expires_in_days
        .map(|days| chrono::Utc::now() + chrono::Duration::days(days as i64));

    let token = state
        .db
        .create_token(user.id, &req.name, &token_hash, &req.scope, project_id, expires_at)
        .await?;

    Ok(Json(CreateTokenResponse {
        token: plaintext_token,
        id: token.id,
        name: token.name,
        scope: token.scope,
        expires_at: token.expires_at,
        created_at: token.created_at,
    }))
}

/// List user's API tokens.
/// Requires unscoped read access (project-scoped tokens cannot list account tokens).
async fn list_tokens(
    AccountAuthUser { user, .. }: AccountAuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<TokenInfo>>, ApiError> {
    let tokens = state.db.list_user_tokens(user.id).await?;

    let token_infos: Vec<TokenInfo> = tokens
        .into_iter()
        .map(|t| TokenInfo {
            id: t.id,
            name: t.name,
            scope: t.scope,
            created_at: t.created_at,
            expires_at: t.expires_at,
            last_used_at: t.last_used_at,
        })
        .collect();

    Ok(Json(token_infos))
}

/// Delete an API token.
/// Requires unscoped read access (project-scoped tokens cannot manage account tokens).
async fn delete_token(
    AccountAuthUser { user, .. }: AccountAuthUser,
    State(state): State<AppState>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> Result<Json<()>, ApiError> {
    let deleted = state.db.delete_token_by_name(user.id, &name).await?;
    if !deleted {
        return Err(ApiError::NotFound("Token not found".to_string()));
    }
    Ok(Json(()))
}

/// Get current user info.
/// Requires unscoped read access (project-scoped tokens cannot access account info).
async fn get_me(AccountAuthUser { user, .. }: AccountAuthUser) -> Json<UserInfo> {
    Json(UserInfo {
        id: user.id,
        username: user.username,
        email: user.email,
        avatar_url: user.avatar_url,
    })
}

// ============================================================================
// Error handling
// ============================================================================

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
    /// 410 Gone - resource has been yanked/retracted
    #[error("{0}")]
    Gone(String),
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

    pub fn gone(msg: impl Into<String>) -> Self {
        Self::Gone(msg.into())
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

        let (status, error_code, message) = match self {
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, "bad_request", msg),
            ApiError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, "unauthorized", msg),
            ApiError::Forbidden(msg) => (StatusCode::FORBIDDEN, "forbidden", msg),
            ApiError::NotFound(msg) => (StatusCode::NOT_FOUND, "not_found", msg),
            ApiError::Conflict(msg) => (StatusCode::CONFLICT, "conflict", msg),
            ApiError::Gone(msg) => (StatusCode::GONE, "gone", msg),
            ApiError::Internal(err) => {
                tracing::error!("Internal error: {:?}", err);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "Internal server error".to_string(),
                )
            }
        };

        let body = ErrorBody::new(error_code, message);

        (status, axum::Json(body)).into_response()
    }
}
