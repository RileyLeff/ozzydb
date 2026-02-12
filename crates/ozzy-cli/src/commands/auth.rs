//! `ozzy auth` — authentication commands.
//!
//! login (GitHub device flow), logout, status, token CRUD.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

// ── Credentials file ─────────────────────────────────────────────

/// Stored credentials.
#[derive(Debug, Serialize, Deserialize)]
pub struct Credentials {
    pub token: String,
    pub registry_url: String,
    pub user: Option<UserInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub username: String,
    pub email: Option<String>,
}

/// Default path: `~/.config/ozzy/credentials.json`.
pub fn credentials_path() -> Result<PathBuf> {
    let config_dir = dirs::config_dir()
        .context("Could not determine config directory")?
        .join("ozzy");
    Ok(config_dir.join("credentials.json"))
}

/// Load credentials from disk.
///
/// Returns `None` if the file doesn't exist or has an unrecognized format
/// (e.g. v1 credentials).
pub fn load_credentials() -> Result<Option<Credentials>> {
    let path = credentials_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let data = std::fs::read_to_string(&path).context("Failed to read credentials file")?;
    match serde_json::from_str::<Credentials>(&data) {
        Ok(creds) => Ok(Some(creds)),
        Err(_) => {
            // Unrecognized format (possibly v1 credentials)
            Ok(None)
        }
    }
}

/// Save credentials to disk (creates parent dirs, restrictive permissions).
fn save_credentials(creds: &Credentials) -> Result<()> {
    let path = credentials_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("Failed to create config directory")?;
    }
    let data = serde_json::to_string_pretty(creds)?;
    std::fs::write(&path, &data).context("Failed to write credentials file")?;

    // Set restrictive permissions on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }

    Ok(())
}

// ── Registry URL helper ──────────────────────────────────────────

/// Get the registry URL from ozzy.toml or fall back to default.
fn get_registry_url() -> String {
    // Try reading ozzy.toml for [remote] section
    if let Ok(toml_str) = std::fs::read_to_string("ozzy.toml") {
        if let Ok(toml) = toml_str.parse::<toml::Table>() {
            if let Some(remote) = toml.get("remote").and_then(|r| r.as_table()) {
                if let Some(url) = remote.get("url").and_then(|u| u.as_str()) {
                    return url.to_string();
                }
            }
        }
    }
    "https://api.ozzydb.com".to_string()
}

// ── Device flow types ────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    #[allow(dead_code)]
    expires_in: u64,
    interval: u64,
}

#[derive(Debug, Serialize)]
struct PollRequest {
    device_code: String,
    client: String,
}

#[derive(Debug, Deserialize)]
struct AuthResponse {
    access_token: Option<String>,
    #[allow(dead_code)]
    token_type: Option<String>,
    user: Option<AuthUserInfo>,
    pending: bool,
}

#[derive(Debug, Deserialize)]
struct AuthUserInfo {
    username: String,
    email: Option<String>,
}

// ── Token types ──────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct CreateTokenRequest {
    name: String,
    scope: String,
    expires_in_days: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct CreateTokenResponse {
    token: String,
    #[allow(dead_code)]
    id: String,
    name: String,
    scope: String,
    expires_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenInfo {
    name: String,
    scope: String,
    created_at: String,
    expires_at: Option<String>,
    last_used_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MeResponse {
    username: String,
    email: Option<String>,
}

// ── Error response ───────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ErrorResponse {
    #[allow(dead_code)]
    error: Option<String>,
    message: Option<String>,
}

// ── Commands ─────────────────────────────────────────────────────

/// `ozzy auth login` — GitHub device flow.
pub async fn login() -> Result<()> {
    let registry_url = get_registry_url();
    let client = reqwest::Client::new();

    // Step 1: Initiate device flow
    let resp = client
        .post(format!("{}/v1/auth/github/device", registry_url))
        .send()
        .await
        .context("Failed to contact registry")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("Failed to initiate login ({}): {}", status, body);
    }

    let device: DeviceCodeResponse = resp.json().await.context("Failed to parse device code")?;

    println!("To authenticate, visit:");
    println!();
    println!("  {}", device.verification_uri);
    println!();
    println!("And enter code: {}", device.user_code);
    println!();
    println!("Waiting for authorization...");

    // Step 2: Poll for completion
    let poll_interval = std::time::Duration::from_secs(device.interval.max(5));
    let poll_req = PollRequest {
        device_code: device.device_code,
        client: "cli-session".to_string(),
    };

    loop {
        tokio::time::sleep(poll_interval).await;

        let resp = client
            .post(format!("{}/v1/auth/github/poll", registry_url))
            .json(&poll_req)
            .send()
            .await
            .context("Failed to poll for auth")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("Auth poll failed ({}): {}", status, body);
        }

        let auth: AuthResponse = resp.json().await.context("Failed to parse auth response")?;

        if auth.pending {
            continue;
        }

        if let Some(token) = auth.access_token {
            let user_info = auth.user.map(|u| UserInfo {
                username: u.username,
                email: u.email,
            });

            let creds = Credentials {
                token,
                registry_url: registry_url.clone(),
                user: user_info.clone(),
            };
            save_credentials(&creds)?;

            if let Some(user) = user_info {
                println!("Logged in as {}", user.username);
            } else {
                println!("Logged in successfully.");
            }
            return Ok(());
        }

        bail!("Authentication failed: no token returned");
    }
}

/// `ozzy auth logout` — remove credentials.
pub fn logout() -> Result<()> {
    let path = credentials_path()?;
    if path.exists() {
        std::fs::remove_file(&path).context("Failed to remove credentials file")?;
        println!("Logged out.");
    } else {
        println!("Not logged in.");
    }
    Ok(())
}

/// `ozzy auth status` — show current user info.
pub async fn status() -> Result<()> {
    let creds = load_credentials()?;
    let Some(creds) = creds else {
        println!("Not logged in. Run `ozzy auth login` to authenticate.");
        return Ok(());
    };

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/v1/auth/me", creds.registry_url))
        .bearer_auth(&creds.token)
        .send()
        .await
        .context("Failed to contact registry")?;

    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        println!("Token expired or invalid. Run `ozzy auth login` to re-authenticate.");
        return Ok(());
    }

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("Failed to get auth status ({}): {}", status, body);
    }

    let me: MeResponse = resp.json().await.context("Failed to parse user info")?;
    println!("Logged in as: {}", me.username);
    if let Some(email) = &me.email {
        println!("Email: {}", email);
    }
    println!("Registry: {}", creds.registry_url);

    Ok(())
}

/// `ozzy auth token create` — create a new API token.
pub async fn token_create(name: &str, scope: &str, expires: Option<u32>) -> Result<()> {
    let creds = load_credentials()?;
    let Some(creds) = creds else {
        bail!("Not logged in. Run `ozzy auth login` first.");
    };

    let req = CreateTokenRequest {
        name: name.to_string(),
        scope: scope.to_string(),
        expires_in_days: expires,
    };

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/v1/auth/token", creds.registry_url))
        .bearer_auth(&creds.token)
        .json(&req)
        .send()
        .await
        .context("Failed to create token")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body: ErrorResponse = resp.json().await.unwrap_or(ErrorResponse {
            error: None,
            message: Some("Unknown error".to_string()),
        });
        bail!(
            "Failed to create token ({}): {}",
            status,
            body.message.unwrap_or_default()
        );
    }

    let token: CreateTokenResponse = resp.json().await.context("Failed to parse token")?;
    println!("Created token: {}", token.name);
    println!("Scope: {}", token.scope);
    if let Some(expires) = &token.expires_at {
        println!("Expires: {}", expires);
    }
    println!();
    println!("Token (shown once — save it now):");
    println!("  {}", token.token);

    Ok(())
}

/// `ozzy auth token ls` — list API tokens.
pub async fn token_list() -> Result<()> {
    let creds = load_credentials()?;
    let Some(creds) = creds else {
        bail!("Not logged in. Run `ozzy auth login` first.");
    };

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/v1/auth/token", creds.registry_url))
        .bearer_auth(&creds.token)
        .send()
        .await
        .context("Failed to list tokens")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("Failed to list tokens ({}): {}", status, body);
    }

    let tokens: Vec<TokenInfo> = resp.json().await.context("Failed to parse token list")?;

    if tokens.is_empty() {
        println!("No API tokens.");
        return Ok(());
    }

    println!(
        "{:<20} {:<25} {:<22} {:<22}",
        "NAME", "SCOPE", "CREATED", "LAST USED"
    );
    for t in &tokens {
        let last_used = t.last_used_at.as_deref().unwrap_or("never");
        println!(
            "{:<20} {:<25} {:<22} {:<22}",
            t.name, t.scope, t.created_at, last_used
        );
    }

    if tokens.iter().any(|t| t.expires_at.is_some()) {
        println!();
        for t in &tokens {
            if let Some(expires) = &t.expires_at {
                println!("  {} expires: {}", t.name, expires);
            }
        }
    }

    Ok(())
}

/// `ozzy auth token revoke` — delete a token by name.
pub async fn token_revoke(name: &str) -> Result<()> {
    let creds = load_credentials()?;
    let Some(creds) = creds else {
        bail!("Not logged in. Run `ozzy auth login` first.");
    };

    let client = reqwest::Client::new();
    let resp = client
        .delete(format!("{}/v1/auth/token/{}", creds.registry_url, name))
        .bearer_auth(&creds.token)
        .send()
        .await
        .context("Failed to revoke token")?;

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        bail!("Token '{}' not found.", name);
    }

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("Failed to revoke token ({}): {}", status, body);
    }

    println!("Revoked token: {}", name);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_credentials_path() {
        let path = credentials_path().unwrap();
        assert!(path.ends_with("ozzy/credentials.json"));
    }

    #[test]
    fn test_save_and_load_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("credentials.json");

        let creds = Credentials {
            token: "ozzy_test_token".to_string(),
            registry_url: "https://api.ozzydb.com".to_string(),
            user: Some(UserInfo {
                username: "testuser".to_string(),
                email: Some("test@example.com".to_string()),
            }),
        };

        // Write
        let data = serde_json::to_string_pretty(&creds).unwrap();
        std::fs::write(&path, &data).unwrap();

        // Read back
        let loaded: Credentials =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(loaded.token, "ozzy_test_token");
        assert_eq!(loaded.registry_url, "https://api.ozzydb.com");
        assert_eq!(loaded.user.unwrap().username, "testuser");
    }

    #[test]
    fn test_credentials_roundtrip_no_user() {
        let creds = Credentials {
            token: "ozzy_abc".to_string(),
            registry_url: "http://localhost:3000".to_string(),
            user: None,
        };
        let json = serde_json::to_string(&creds).unwrap();
        let loaded: Credentials = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.token, "ozzy_abc");
        assert!(loaded.user.is_none());
    }
}
