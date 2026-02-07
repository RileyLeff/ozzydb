//! V1 API endpoints.

mod access;
mod auth;
mod projects;
mod push_pull;

use axum::Router;

use crate::AppState;

/// Build the v1 API router.
pub fn router() -> Router<AppState> {
    Router::new()
        .nest("/auth", auth::router())
        .merge(projects::router())
        .merge(push_pull::router())
}
