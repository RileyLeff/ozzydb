//! V1 API endpoints.

mod access;
pub mod auth;
mod collections;
mod data;
mod secrets;

use axum::Router;

use crate::AppState;

/// Build the v1 API router.
pub fn router() -> Router<AppState> {
    Router::new()
        .nest("/auth", auth::router())
        .nest("/data", data::router())
        .nest("/collections", collections::router())
        .nest("/secrets", secrets::router())
}
