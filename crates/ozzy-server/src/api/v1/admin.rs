//! Admin API endpoints (requires is_admin flag on user).

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::AppState;
use crate::auth::middleware::AdminUser;

use super::auth::ApiError;

// ============================================================================
// Wire types
// ============================================================================

#[derive(Serialize)]
pub struct AdminJobResponse {
    pub id: Uuid,
    pub project_id: Uuid,
    pub endpoint_name: String,
    pub commit_id: Uuid,
    pub status: String,
    pub error_message: Option<String>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
pub struct ListJobsQuery {
    pub status: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Serialize)]
pub struct RateLimitResponse {
    pub global_max_concurrent: u32,
    pub per_user_max_concurrent: u32,
    pub active_jobs_global: i64,
}

#[derive(Serialize)]
pub struct AdminUserResponse {
    pub id: Uuid,
    pub username: String,
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    pub is_admin: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct ListUsersQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Serialize)]
pub struct CancelJobResponse {
    pub cancelled: bool,
}

// ============================================================================
// Routes
// ============================================================================

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/jobs", get(list_jobs))
        .route("/jobs/{id}/cancel", post(cancel_job))
        .route("/rate-limits", get(get_rate_limits))
        .route("/users", get(list_users))
}

// ============================================================================
// Handlers
// ============================================================================

async fn list_jobs(
    _admin: AdminUser,
    State(state): State<AppState>,
    Query(query): Query<ListJobsQuery>,
) -> Result<Json<Vec<AdminJobResponse>>, ApiError> {
    let limit = query.limit.unwrap_or(50).max(1).min(200);

    // Validate status filter if provided
    if let Some(ref status) = query.status {
        match status.as_str() {
            "queued" | "running" | "done" | "failed" => {}
            _ => {
                return Err(ApiError::BadRequest(format!(
                    "Unknown job status: {status}"
                )));
            }
        }
    }

    let jobs = state
        .db
        .list_jobs_global(query.status.as_deref(), limit)
        .await
        .map_err(ApiError::Internal)?;

    let responses: Vec<AdminJobResponse> = jobs
        .into_iter()
        .map(|j| AdminJobResponse {
            id: j.id,
            project_id: j.project_id,
            endpoint_name: j.endpoint_name,
            commit_id: j.commit_id,
            status: j.status,
            error_message: j.error_message,
            created_by: j.created_by,
            created_at: j.created_at,
            started_at: j.started_at,
            completed_at: j.completed_at,
        })
        .collect();

    Ok(Json(responses))
}

async fn cancel_job(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<CancelJobResponse>, ApiError> {
    let cancelled = state.db.cancel_job(id).await.map_err(ApiError::Internal)?;

    Ok(Json(CancelJobResponse { cancelled }))
}

async fn get_rate_limits(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<RateLimitResponse>, ApiError> {
    let active = state
        .db
        .count_active_jobs_global()
        .await
        .map_err(ApiError::Internal)?;

    Ok(Json(RateLimitResponse {
        global_max_concurrent: state.config.rate_limit.global_max_concurrent,
        per_user_max_concurrent: state.config.rate_limit.per_user_max_concurrent,
        active_jobs_global: active,
    }))
}

async fn list_users(
    _admin: AdminUser,
    State(state): State<AppState>,
    Query(query): Query<ListUsersQuery>,
) -> Result<Json<Vec<AdminUserResponse>>, ApiError> {
    let limit = query.limit.unwrap_or(50).max(1).min(200);
    let offset = query.offset.unwrap_or(0).max(0);
    let users = state
        .db
        .list_users(limit, offset)
        .await
        .map_err(ApiError::Internal)?;

    let responses: Vec<AdminUserResponse> = users
        .into_iter()
        .map(|u| AdminUserResponse {
            id: u.id,
            username: u.username,
            display_name: u.display_name,
            email: u.email,
            avatar_url: u.avatar_url,
            is_admin: u.is_admin,
            created_at: u.created_at,
        })
        .collect();

    Ok(Json(responses))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_jobs_query_defaults() {
        let q = ListJobsQuery {
            status: None,
            limit: None,
        };
        assert_eq!(q.limit.unwrap_or(50).max(1).min(200), 50);
    }

    #[test]
    fn test_list_jobs_query_clamp_upper() {
        let q = ListJobsQuery {
            status: Some("running".to_string()),
            limit: Some(500),
        };
        assert_eq!(q.limit.unwrap_or(50).max(1).min(200), 200);
    }

    #[test]
    fn test_list_jobs_query_clamp_negative() {
        let q = ListJobsQuery {
            status: None,
            limit: Some(-1),
        };
        assert_eq!(q.limit.unwrap_or(50).max(1).min(200), 1);
    }

    #[test]
    fn test_list_users_query_defaults() {
        let q = ListUsersQuery {
            limit: None,
            offset: None,
        };
        assert_eq!(q.limit.unwrap_or(50).max(1).min(200), 50);
        assert_eq!(q.offset.unwrap_or(0).max(0), 0);
    }

    #[test]
    fn test_list_users_negative_offset_clamped() {
        let q = ListUsersQuery {
            limit: Some(10),
            offset: Some(-5),
        };
        assert_eq!(q.offset.unwrap_or(0).max(0), 0);
    }

    #[test]
    fn test_list_users_negative_limit_clamped() {
        let q = ListUsersQuery {
            limit: Some(-10),
            offset: None,
        };
        assert_eq!(q.limit.unwrap_or(50).max(1).min(200), 1);
    }
}
