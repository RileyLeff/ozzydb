//! REST API handlers.

pub mod v1;

use axum::{routing::get, Json, Router};
use serde::Serialize;

use crate::AppState;

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}

/// Build the complete API router.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .nest("/api/v1", v1::router())
}
