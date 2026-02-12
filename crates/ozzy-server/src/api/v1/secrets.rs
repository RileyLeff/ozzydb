//! Secrets management API endpoints.
//!
//! Secrets are encrypted at rest with AES-256-GCM. Values are write-only:
//! they can be set and deleted but never read back through the API.
//! The server decrypts them only when injecting into compute containers.

use aes_gcm::{Aes256Gcm, KeyInit, Nonce, aead::Aead};
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::access::enforce_write_access;
use super::auth::ApiError;
use crate::{AppState, auth::middleware::AuthUser};

// ============================================================================
// Wire types
// ============================================================================

#[derive(Deserialize)]
struct SetSecretRequest {
    name: String,
    value: String,
}

#[derive(Serialize)]
struct SetSecretResponse {
    name: String,
    version_id: Uuid,
    created: bool,
}

#[derive(Serialize)]
struct SecretListItem {
    name: String,
    version_id: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

// ============================================================================
// Crypto helpers
// ============================================================================

/// Encrypt a plaintext value with AES-256-GCM.
/// Returns nonce (12 bytes) || ciphertext.
fn encrypt_secret(key: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, ApiError> {
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("Invalid encryption key: {}", e)))?;

    let mut nonce_bytes = [0u8; 12];
    rand::rng().fill(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("Encryption failed: {}", e)))?;

    // Prepend nonce to ciphertext for storage
    let mut result = Vec::with_capacity(12 + ciphertext.len());
    result.extend_from_slice(&nonce_bytes);
    result.extend_from_slice(&ciphertext);
    Ok(result)
}

/// Get the encryption key from config, or return an error if not configured.
fn require_encryption_key(state: &AppState) -> Result<&[u8], ApiError> {
    state
        .config
        .secrets_encryption_key
        .as_deref()
        .ok_or_else(|| {
            ApiError::Internal(anyhow::anyhow!(
                "SECRETS_ENCRYPTION_KEY not configured — secrets API is disabled"
            ))
        })
}

/// Valid name pattern: [a-zA-Z0-9_-]+
fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

// ============================================================================
// Routes
// ============================================================================

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/{owner}/{project}", post(set_secret))
        .route("/{owner}/{project}", get(list_secrets))
        .route(
            "/{owner}/{project}/{name}",
            axum::routing::delete(delete_secret),
        )
}

/// Set (upsert) a secret. The value is encrypted and stored.
/// A new version_id is generated on every call, even if the value is unchanged.
async fn set_secret(
    AuthUser { user, scope }: AuthUser,
    State(state): State<AppState>,
    Path((owner, project_slug)): Path<(String, String)>,
    Json(req): Json<SetSecretRequest>,
) -> Result<Json<SetSecretResponse>, ApiError> {
    let key = require_encryption_key(&state)?;

    if !is_valid_name(&req.name) {
        return Err(ApiError::bad_request(format!(
            "Invalid secret name '{}': must match [a-zA-Z0-9_-]+",
            req.name
        )));
    }

    if req.value.is_empty() {
        return Err(ApiError::bad_request("Secret value cannot be empty"));
    }

    let project = state
        .db
        .get_project(&owner, &project_slug)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Project '{}/{}'", owner, project_slug)))?;

    enforce_write_access(&state, &project, &owner, &project_slug, &user, &scope).await?;

    // Check if this is creating or updating
    let existing = state.db.get_secret(project.id, &req.name).await?;
    let created = existing.is_none();

    // Encrypt the value
    let encrypted = encrypt_secret(key, req.value.as_bytes())?;

    let secret = state
        .db
        .upsert_secret(project.id, &req.name, &encrypted, user.id)
        .await?;

    Ok(Json(SetSecretResponse {
        name: secret.name,
        version_id: secret.version_id,
        created,
    }))
}

/// List secret names and metadata (values are never returned).
/// Requires write access — secret metadata should not be publicly enumerable.
async fn list_secrets(
    AuthUser { user, scope }: AuthUser,
    State(state): State<AppState>,
    Path((owner, project_slug)): Path<(String, String)>,
) -> Result<Json<Vec<SecretListItem>>, ApiError> {
    let project = state
        .db
        .get_project(&owner, &project_slug)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Project '{}/{}'", owner, project_slug)))?;

    enforce_write_access(&state, &project, &owner, &project_slug, &user, &scope).await?;

    let secrets = state.db.list_secrets(project.id).await?;

    let items: Vec<SecretListItem> = secrets
        .into_iter()
        .map(|s| SecretListItem {
            name: s.name,
            version_id: s.version_id,
            created_at: s.created_at,
            updated_at: s.updated_at,
        })
        .collect();

    Ok(Json(items))
}

/// Delete a secret.
async fn delete_secret(
    AuthUser { user, scope }: AuthUser,
    State(state): State<AppState>,
    Path((owner, project_slug, name)): Path<(String, String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let project = state
        .db
        .get_project(&owner, &project_slug)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Project '{}/{}'", owner, project_slug)))?;

    enforce_write_access(&state, &project, &owner, &project_slug, &user, &scope).await?;

    let deleted = state.db.delete_secret(project.id, &name).await?;
    if !deleted {
        return Err(ApiError::not_found(format!("Secret '{}'", name)));
    }

    Ok(Json(serde_json::json!({ "deleted": true, "name": name })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_roundtrip() {
        let key = [0x42u8; 32];
        let plaintext = b"my-secret-value";

        let encrypted = encrypt_secret(&key, plaintext).unwrap();
        assert!(encrypted.len() > 12, "Should have nonce + ciphertext");

        // Decrypt to verify
        let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
        let nonce = Nonce::from_slice(&encrypted[..12]);
        let decrypted = cipher.decrypt(nonce, &encrypted[12..]).unwrap();
        assert_eq!(&decrypted, plaintext);
    }

    #[test]
    fn test_encrypt_different_nonces() {
        let key = [0x42u8; 32];
        let plaintext = b"same-value";

        let enc1 = encrypt_secret(&key, plaintext).unwrap();
        let enc2 = encrypt_secret(&key, plaintext).unwrap();

        // Different nonces should produce different ciphertexts
        assert_ne!(enc1, enc2);
    }

    #[test]
    fn test_is_valid_name() {
        assert!(is_valid_name("GEMINI_API_KEY"));
        assert!(is_valid_name("secret-1"));
        assert!(is_valid_name("abc"));
        assert!(!is_valid_name(""));
        assert!(!is_valid_name("has space"));
        assert!(!is_valid_name("has.dot"));
    }
}
