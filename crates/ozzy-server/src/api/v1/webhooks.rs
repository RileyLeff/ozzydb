//! Webhook handlers for external service events.
//!
//! Currently handles GitHub App installation events.

use axum::{Router, extract::State, http::HeaderMap, routing::post};
use bytes::Bytes;
use serde::Deserialize;

use super::auth::ApiError;
use crate::AppState;
use crate::git::github::verify_webhook_signature;

/// Build the webhooks router.
pub fn router() -> Router<AppState> {
    Router::new().route("/github", post(github_webhook))
}

/// GitHub webhook installation event payload.
#[derive(Debug, Deserialize)]
struct InstallationEvent {
    action: String,
    installation: InstallationPayload,
}

#[derive(Debug, Deserialize)]
struct InstallationPayload {
    id: i64,
    account: InstallationAccount,
}

#[derive(Debug, Deserialize)]
struct InstallationAccount {
    login: String,
    #[serde(rename = "type")]
    account_type: String,
}

/// Handle GitHub webhook events.
///
/// Processes installation events (created/deleted) to track which
/// users/orgs have installed the OzzyDB GitHub App.
async fn github_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<&'static str, ApiError> {
    // Verify webhook signature if secret is configured
    if let Some(secret) = state.git.webhook_secret() {
        let signature = headers
            .get("x-hub-signature-256")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| {
                ApiError::Unauthorized("Missing X-Hub-Signature-256 header".to_string())
            })?;

        if !verify_webhook_signature(&body, secret, signature) {
            return Err(ApiError::Unauthorized(
                "Invalid webhook signature".to_string(),
            ));
        }
    } else {
        tracing::warn!("GitHub webhook secret not configured — skipping signature verification");
    }

    // Check event type
    let event_type = headers
        .get("x-github-event")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    match event_type {
        "installation" => handle_installation_event(&state, &body).await,
        "ping" => {
            tracing::info!("Received GitHub webhook ping");
            Ok("pong")
        }
        _ => {
            tracing::debug!("Ignoring GitHub webhook event: {}", event_type);
            Ok("ignored")
        }
    }
}

/// Handle GitHub App installation events.
async fn handle_installation_event(
    state: &AppState,
    body: &[u8],
) -> Result<&'static str, ApiError> {
    let event: InstallationEvent = serde_json::from_slice(body)
        .map_err(|e| ApiError::BadRequest(format!("Invalid JSON: {}", e)))?;

    match event.action.as_str() {
        "created" => {
            tracing::info!(
                "GitHub App installed by {} ({}), installation_id={}",
                event.installation.account.login,
                event.installation.account.account_type,
                event.installation.id
            );

            state
                .db
                .upsert_github_installation(
                    event.installation.id,
                    &event.installation.account.account_type,
                    &event.installation.account.login,
                )
                .await
                .map_err(ApiError::Internal)?;

            Ok("installed")
        }
        "deleted" => {
            tracing::info!(
                "GitHub App uninstalled by {}, installation_id={}",
                event.installation.account.login,
                event.installation.id
            );

            state
                .db
                .delete_github_installation(event.installation.id)
                .await
                .map_err(ApiError::Internal)?;

            Ok("uninstalled")
        }
        action => {
            tracing::debug!("Ignoring installation action: {}", action);
            Ok("ignored")
        }
    }
}
