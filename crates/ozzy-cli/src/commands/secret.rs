//! `ozzy secret` — manage project secrets (encrypted key-value pairs).
//!
//! Secrets are write-only: values can be set and deleted but never read back.
//! The server decrypts them only when injecting into compute containers.

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use super::shared;

// ── Response types ──────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct SetSecretResponse {
    name: String,
    created: bool,
}

#[derive(Debug, Deserialize)]
struct SecretListItem {
    name: String,
    created_at: String,
    updated_at: String,
}

// ── Commands ────────────────────────────────────────────────────

/// Set (upsert) a secret. Prompts for the value on stdin.
pub async fn set(name: &str) -> Result<()> {
    let creds = shared::require_auth()?;
    let project = shared::load_project_from_toml()?;
    let registry_url = shared::registry_url(&creds);
    let client = shared::http_client()?;

    // Read secret value from stdin
    eprint!("Enter secret value for {}: ", name);
    let mut value = String::new();
    std::io::stdin()
        .read_line(&mut value)
        .context("Failed to read secret value")?;
    let value = value.trim_end();

    if value.is_empty() {
        bail!("Secret value cannot be empty");
    }

    let body = serde_json::json!({
        "name": name,
        "value": value,
    });

    let resp = client
        .post(format!(
            "{}/api/v1/secrets/{}/{}",
            registry_url, project.owner, project.slug
        ))
        .bearer_auth(&creds.token)
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let err = shared::extract_error(resp).await;
        bail!("Failed to set secret '{}': {}", name, err);
    }

    let result: SetSecretResponse = resp.json().await?;
    if result.created {
        println!("Created secret '{}'.", result.name);
    } else {
        println!("Updated secret '{}'.", result.name);
    }

    Ok(())
}

/// List secrets (names only — values are never exposed).
pub async fn ls() -> Result<()> {
    let creds = shared::require_auth()?;
    let project = shared::load_project_from_toml()?;
    let registry_url = shared::registry_url(&creds);
    let client = shared::http_client()?;

    let resp = client
        .get(format!(
            "{}/api/v1/secrets/{}/{}",
            registry_url, project.owner, project.slug
        ))
        .bearer_auth(&creds.token)
        .send()
        .await?;

    if !resp.status().is_success() {
        let err = shared::extract_error(resp).await;
        bail!("Failed to list secrets: {}", err);
    }

    let secrets: Vec<SecretListItem> = resp.json().await?;

    if secrets.is_empty() {
        println!("No secrets. Set one with `ozzy secret set <name>`.");
        return Ok(());
    }

    println!("{:<24} {:<22} {}", "NAME", "CREATED", "UPDATED");
    for s in &secrets {
        println!(
            "{:<24} {:<22} {}",
            s.name,
            &s.created_at[..10],
            &s.updated_at[..10],
        );
    }

    Ok(())
}

/// Delete a secret.
pub async fn rm(name: &str) -> Result<()> {
    let creds = shared::require_auth()?;
    let project = shared::load_project_from_toml()?;
    let registry_url = shared::registry_url(&creds);
    let client = shared::http_client()?;

    let resp = client
        .delete(format!(
            "{}/api/v1/secrets/{}/{}/{}",
            registry_url, project.owner, project.slug, name
        ))
        .bearer_auth(&creds.token)
        .send()
        .await?;

    if !resp.status().is_success() {
        let err = shared::extract_error(resp).await;
        bail!("Failed to delete secret '{}': {}", name, err);
    }

    println!("Deleted secret '{}'.", name);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_secret_response_deserialization() {
        let json = serde_json::json!({
            "name": "GEMINI_API_KEY",
            "version_id": "00000000-0000-0000-0000-000000000000",
            "created": true,
        });
        let resp: SetSecretResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.name, "GEMINI_API_KEY");
        assert!(resp.created);
    }

    #[test]
    fn test_secret_list_deserialization() {
        let json = serde_json::json!([{
            "name": "API_KEY",
            "version_id": "00000000-0000-0000-0000-000000000000",
            "created_at": "2025-01-01T00:00:00Z",
            "updated_at": "2025-01-02T00:00:00Z",
        }]);
        let secrets: Vec<SecretListItem> = serde_json::from_value(json).unwrap();
        assert_eq!(secrets.len(), 1);
        assert_eq!(secrets[0].name, "API_KEY");
    }
}
