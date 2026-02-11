//! V1 API endpoints.
//!
//! v2 cleanup: projects and push_pull handlers removed.
//! Only auth endpoints survive from v1.

mod access;
pub mod auth;

use axum::Router;

use crate::AppState;

/// Build the v1 API router.
pub fn router() -> Router<AppState> {
    Router::new().nest("/auth", auth::router())
}
