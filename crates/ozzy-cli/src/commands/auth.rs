//! Authentication commands for registry login/logout and token management.

use anyhow::{Context, Result};
use ozzy_core::registry::{CredentialsFile, RegistryClient, RegistryCredentials};
use std::path::PathBuf;
use std::time::Duration;

/// Get the path to the credentials file.
fn credentials_path() -> Result<PathBuf> {
    let config_dir = dirs::config_dir()
        .context("Could not determine config directory")?
        .join("ozzy");
    std::fs::create_dir_all(&config_dir)?;
    Ok(config_dir.join("credentials.json"))
}

/// Load credentials file.
fn load_credentials() -> Result<CredentialsFile> {
    let path = credentials_path()?;
    if path.exists() {
        let content = std::fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&content)?)
    } else {
        Ok(CredentialsFile::default())
    }
}

/// Save credentials file with restrictive permissions.
fn save_credentials(creds: &CredentialsFile) -> Result<()> {
    let path = credentials_path()?;
    let content = serde_json::to_string_pretty(creds)?;
    std::fs::write(&path, content)?;

    // Set file permissions to 0600 (owner read/write only) on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(&path, perms)?;
    }

    Ok(())
}

/// Get the default registry URL.
fn default_registry() -> String {
    std::env::var("OZZY_REGISTRY").unwrap_or_else(|_| "https://registry.ozzydb.dev".to_string())
}

/// Login to a registry using GitHub device flow.
pub async fn login(registry: Option<&str>) -> Result<()> {
    let registry_url = registry
        .map(|s| s.to_string())
        .unwrap_or_else(default_registry);

    println!("Logging in to {}...", registry_url);

    let client = RegistryClient::new(&registry_url);

    // Initiate device flow
    let device_response = client.auth_device_flow().await?;

    println!();
    println!("Please visit: {}", device_response.verification_uri);
    println!("And enter code: {}", device_response.user_code);
    println!();
    println!("Waiting for authorization...");

    // Poll for completion (with slow_down backoff support)
    let mut interval = Duration::from_secs(device_response.interval);
    let deadline = std::time::Instant::now() + Duration::from_secs(device_response.expires_in);

    loop {
        if std::time::Instant::now() > deadline {
            anyhow::bail!("Device code expired. Please try again.");
        }

        tokio::time::sleep(interval).await;

        let auth_response = match client.auth_poll(&device_response.device_code).await {
            Ok(resp) => resp,
            Err(e) => {
                // Check if this is a slow_down response
                let err_str = e.to_string();
                if err_str.contains("slow_down") {
                    interval += Duration::from_secs(5);
                    continue;
                }
                return Err(e);
            }
        };

        if !auth_response.pending {
            if let (Some(token), Some(user)) = (auth_response.access_token, auth_response.user) {
                // Save credentials
                let mut creds = load_credentials()?;
                creds.set(
                    registry_url.clone(),
                    RegistryCredentials {
                        access_token: token,
                        username: user.username.clone(),
                        expires_at: None,
                    },
                );
                if creds.default.is_none() {
                    creds.default = Some(registry_url.clone());
                }
                save_credentials(&creds)?;

                println!();
                println!("Logged in as {} to {}", user.username, registry_url);
                return Ok(());
            } else {
                anyhow::bail!("Authentication failed: no token received");
            }
        }
    }
}

/// Logout from a registry.
pub async fn logout(registry: Option<&str>) -> Result<()> {
    let registry_url = registry
        .map(|s| s.to_string())
        .unwrap_or_else(default_registry);

    let mut creds = load_credentials()?;

    if creds.remove(&registry_url).is_some() {
        // If this was the default, clear it
        if creds.default.as_ref() == Some(&registry_url) {
            creds.default = None;
        }
        save_credentials(&creds)?;
        println!("Logged out from {}", registry_url);
    } else {
        println!("Not logged in to {}", registry_url);
    }

    Ok(())
}

/// Create a new API token.
pub async fn token_create(
    name: &str,
    scopes: &[String],
    expires_days: Option<u32>,
    registry: Option<&str>,
) -> Result<()> {
    let registry_url = registry
        .map(|s| s.to_string())
        .unwrap_or_else(default_registry);

    let creds = load_credentials()?;
    let registry_creds = creds
        .get(&registry_url)
        .context("Not logged in. Run 'ozzy auth login' first.")?;

    let client = RegistryClient::with_token(&registry_url, &registry_creds.access_token);

    let token_response = client.create_token(name, scopes, expires_days).await?;

    println!("Created token: {}", name);
    println!();
    println!("Token: {}", token_response.token);
    println!();
    println!("Save this token securely - it won't be shown again.");

    Ok(())
}

/// List API tokens.
pub async fn token_list(registry: Option<&str>) -> Result<()> {
    let registry_url = registry
        .map(|s| s.to_string())
        .unwrap_or_else(default_registry);

    let creds = load_credentials()?;
    let registry_creds = creds
        .get(&registry_url)
        .context("Not logged in. Run 'ozzy auth login' first.")?;

    let client = RegistryClient::with_token(&registry_url, &registry_creds.access_token);

    let tokens = client.list_tokens().await?;

    if tokens.is_empty() {
        println!("No API tokens found.");
        return Ok(());
    }

    println!("API Tokens for {}:", registry_url);
    println!();
    for token in tokens {
        println!("  {} (scopes: {})", token.name, token.scopes.join(", "));
        if let Some(expires) = token.expires_at {
            println!("    Expires: {}", expires);
        }
        if let Some(last_used) = token.last_used_at {
            println!("    Last used: {}", last_used);
        }
    }

    Ok(())
}

/// Revoke an API token.
pub async fn token_revoke(name: &str, registry: Option<&str>) -> Result<()> {
    let registry_url = registry
        .map(|s| s.to_string())
        .unwrap_or_else(default_registry);

    let creds = load_credentials()?;
    let registry_creds = creds
        .get(&registry_url)
        .context("Not logged in. Run 'ozzy auth login' first.")?;

    let client = RegistryClient::with_token(&registry_url, &registry_creds.access_token);

    client.revoke_token(name).await?;

    println!("Revoked token: {}", name);

    Ok(())
}
