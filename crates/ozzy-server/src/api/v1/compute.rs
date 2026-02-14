//! Compute provider info endpoints.

use axum::{Json, Router, extract::State, routing::get};

use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/providers", get(list_providers))
}

/// GET /v1/compute/providers — list active compute providers.
async fn list_providers(State(state): State<AppState>) -> Json<serde_json::Value> {
    let providers: Vec<serde_json::Value> = state
        .compute
        .provider_names()
        .into_iter()
        .map(|name| serde_json::json!({ "name": name }))
        .collect();

    Json(serde_json::json!({
        "default": state.compute.default_provider(),
        "providers": providers,
    }))
}
