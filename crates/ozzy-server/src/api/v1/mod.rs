//! V1 API endpoints.

mod access;
pub mod auth;
mod collections;
mod commits;
mod data;
mod endpoints;
mod fetch;
mod projects;
mod push;
mod secrets;
mod webhooks;

use axum::Router;

use crate::AppState;

/// Build the v1 API router.
pub fn router() -> Router<AppState> {
    Router::new()
        .nest("/auth", auth::router())
        .nest("/data", data::router())
        .nest("/collections", collections::router())
        .nest("/secrets", secrets::router())
        .nest("/push", push::router())
        .nest("/projects", projects::router())
        .nest("/endpoints", endpoints::router())
        .nest("/webhooks", webhooks::router())
        .nest("/fetch", fetch::router())
        .nest("/commits", commits::router())
}
