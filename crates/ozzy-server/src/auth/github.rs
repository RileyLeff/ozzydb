//! GitHub OAuth device flow implementation.
//!
//! Implements GitHub's device flow for CLI authentication.
//! https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps#device-flow

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::config::Config;

/// GitHub user profile from API.
#[derive(Debug, Clone, Deserialize)]
pub struct GitHubUser {
    pub id: i64,
    pub login: String,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
}

/// GitHub device code response.
#[derive(Debug, Clone, Deserialize)]
pub struct GitHubDeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

/// GitHub access token response.
#[derive(Debug, Clone, Deserialize)]
pub struct GitHubTokenResponse {
    pub access_token: Option<String>,
    pub token_type: Option<String>,
    pub scope: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

/// Initiate GitHub device flow.
pub async fn initiate_device_flow(config: &Config) -> Result<GitHubDeviceCodeResponse> {
    let client = reqwest::Client::new();

    let response = client
        .post("https://github.com/login/device/code")
        .header("Accept", "application/json")
        .form(&[
            ("client_id", config.github_client_id.as_str()),
            ("scope", "read:user user:email"),
        ])
        .send()
        .await
        .context("Failed to initiate device flow")?;

    if !response.status().is_success() {
        let text = response
            .text()
            .await
            .context("Failed to read GitHub device flow error body")?;
        anyhow::bail!("GitHub device flow failed: {}", text);
    }

    let device_response: GitHubDeviceCodeResponse = response
        .json()
        .await
        .context("Failed to parse device code response")?;

    Ok(device_response)
}

/// Poll for device flow token.
/// Returns Some(token) when auth is complete, None if still pending.
pub async fn poll_device_flow(config: &Config, device_code: &str) -> Result<Option<String>> {
    let client = reqwest::Client::new();

    let response = client
        .post("https://github.com/login/oauth/access_token")
        .header("Accept", "application/json")
        .form(&[
            ("client_id", config.github_client_id.as_str()),
            ("client_secret", config.github_client_secret.as_str()),
            ("device_code", device_code),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ])
        .send()
        .await
        .context("Failed to poll device flow")?;

    let token_response: GitHubTokenResponse = response
        .json()
        .await
        .context("Failed to parse token response")?;

    // Check for errors
    if let Some(error) = &token_response.error {
        match error.as_str() {
            "authorization_pending" => return Ok(None),
            "slow_down" => return Ok(None),
            "expired_token" => anyhow::bail!("Device code expired"),
            "access_denied" => anyhow::bail!("Access denied by user"),
            _ => anyhow::bail!(
                "GitHub OAuth error: {} - {:?}",
                error,
                token_response.error_description
            ),
        }
    }

    // Return access token if present
    Ok(token_response.access_token)
}

/// Fetch GitHub user profile using access token.
pub async fn get_github_user(access_token: &str) -> Result<GitHubUser> {
    let client = reqwest::Client::new();
    let user: GitHubUser = client
        .get("https://api.github.com/user")
        .header("Authorization", format!("Bearer {}", access_token))
        .header("User-Agent", "OzzyDB-Server")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .context("Failed to fetch GitHub user")?
        .json()
        .await
        .context("Failed to parse GitHub user response")?;

    Ok(user)
}
