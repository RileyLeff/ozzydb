//! HTTP client for interacting with OzzyDB registry servers.

use anyhow::{Context, Result};
use reqwest::multipart;
use std::collections::HashMap;

use super::protocol::*;

/// Registry client for communicating with the server.
#[derive(Clone)]
pub struct RegistryClient {
    base_url: String,
    access_token: Option<String>,
    client: reqwest::Client,
}

impl RegistryClient {
    /// Create a new registry client.
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            access_token: None,
            client: reqwest::Client::new(),
        }
    }

    /// Create a client with authentication.
    pub fn with_token(base_url: &str, token: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            access_token: Some(token.to_string()),
            client: reqwest::Client::new(),
        }
    }

    /// Set the access token.
    pub fn set_token(&mut self, token: &str) {
        self.access_token = Some(token.to_string());
    }

    fn auth_header(&self) -> Option<String> {
        self.access_token.as_ref().map(|t| format!("Bearer {}", t))
    }

    // ========================================================================
    // Authentication
    // ========================================================================

    /// Initiate GitHub device flow.
    pub async fn auth_device_flow(&self) -> Result<DeviceCodeResponse> {
        let response = self
            .client
            .post(format!("{}/api/v1/auth/github/device", self.base_url))
            .send()
            .await
            .context("Failed to initiate device flow")?;

        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("Device flow failed: {}", text);
        }

        response
            .json()
            .await
            .context("Failed to parse device code response")
    }

    /// Poll for device flow completion.
    pub async fn auth_poll(&self, device_code: &str) -> Result<AuthResponse> {
        let response = self
            .client
            .post(format!("{}/api/v1/auth/github/poll", self.base_url))
            .json(&serde_json::json!({ "device_code": device_code }))
            .send()
            .await
            .context("Failed to poll device flow")?;

        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("Poll failed: {}", text);
        }

        response
            .json()
            .await
            .context("Failed to parse auth response")
    }

    /// Get current user info.
    pub async fn get_me(&self) -> Result<UserInfo> {
        let mut request = self.client.get(format!("{}/api/v1/auth/me", self.base_url));

        if let Some(auth) = self.auth_header() {
            request = request.header("Authorization", auth);
        }

        let response = request.send().await.context("Failed to get user info")?;

        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("Get user failed: {}", text);
        }

        response.json().await.context("Failed to parse user info")
    }

    /// Create a new API token.
    pub async fn create_token(
        &self,
        name: &str,
        scopes: &[String],
        expires_in_days: Option<u32>,
    ) -> Result<CreateTokenResponse> {
        let mut request = self
            .client
            .post(format!("{}/api/v1/auth/token", self.base_url))
            .json(&CreateTokenRequest {
                name: name.to_string(),
                scopes: scopes.to_vec(),
                expires_in_days,
            });

        if let Some(auth) = self.auth_header() {
            request = request.header("Authorization", auth);
        }

        let response = request.send().await.context("Failed to create token")?;

        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("Create token failed: {}", text);
        }

        response
            .json()
            .await
            .context("Failed to parse token response")
    }

    /// List user's tokens.
    pub async fn list_tokens(&self) -> Result<Vec<TokenInfo>> {
        let mut request = self
            .client
            .get(format!("{}/api/v1/auth/token", self.base_url));

        if let Some(auth) = self.auth_header() {
            request = request.header("Authorization", auth);
        }

        let response = request.send().await.context("Failed to list tokens")?;

        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("List tokens failed: {}", text);
        }

        response.json().await.context("Failed to parse tokens")
    }

    /// Revoke a token by name.
    pub async fn revoke_token(&self, name: &str) -> Result<()> {
        let mut request = self
            .client
            .delete(format!("{}/api/v1/auth/token/{}", self.base_url, name));

        if let Some(auth) = self.auth_header() {
            request = request.header("Authorization", auth);
        }

        let response = request.send().await.context("Failed to revoke token")?;

        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("Revoke token failed: {}", text);
        }

        Ok(())
    }

    // ========================================================================
    // Projects
    // ========================================================================

    /// List user's projects.
    pub async fn list_projects(&self) -> Result<Vec<ProjectInfo>> {
        let mut request = self
            .client
            .get(format!("{}/api/v1/projects", self.base_url));

        if let Some(auth) = self.auth_header() {
            request = request.header("Authorization", auth);
        }

        let response = request.send().await.context("Failed to list projects")?;

        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("List projects failed: {}", text);
        }

        response.json().await.context("Failed to parse projects")
    }

    /// Get project info.
    pub async fn get_project(&self, owner: &str, slug: &str) -> Result<ProjectInfo> {
        let response = self
            .client
            .get(format!("{}/api/v1/{}/{}", self.base_url, owner, slug))
            .send()
            .await
            .context("Failed to get project")?;

        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("Get project failed: {}", text);
        }

        response.json().await.context("Failed to parse project")
    }

    /// List project commits.
    pub async fn list_commits(
        &self,
        owner: &str,
        project: &str,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> Result<Vec<CommitInfo>> {
        let mut url = format!("{}/api/v1/{}/{}/commits", self.base_url, owner, project);
        let mut query = Vec::new();
        if let Some(l) = limit {
            query.push(format!("limit={}", l));
        }
        if let Some(o) = offset {
            query.push(format!("offset={}", o));
        }
        if !query.is_empty() {
            url.push('?');
            url.push_str(&query.join("&"));
        }

        let mut request = self.client.get(&url);
        if let Some(auth) = self.auth_header() {
            request = request.header("Authorization", auth);
        }

        let response = request.send().await.context("Failed to list commits")?;
        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("List commits failed: {}", text);
        }

        response
            .json()
            .await
            .context("Failed to parse commits response")
    }

    // ========================================================================
    // Push/Pull
    // ========================================================================

    /// Check which content already exists on the server.
    pub async fn check_content(
        &self,
        owner: &str,
        project: &str,
        data_hashes: &HashMap<String, String>,
        transform_hashes: &HashMap<String, String>,
    ) -> Result<ContentCheckResponse> {
        let mut request = self.client.post(format!(
            "{}/api/v1/{}/{}/content/check",
            self.base_url, owner, project
        ));

        if let Some(auth) = self.auth_header() {
            request = request.header("Authorization", auth);
        }

        let response = request
            .json(&ContentCheckRequest {
                data_hashes: data_hashes.clone(),
                transform_hashes: transform_hashes.clone(),
            })
            .send()
            .await
            .context("Failed to check content")?;

        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("Content check failed: {}", text);
        }

        response
            .json()
            .await
            .context("Failed to parse content check response")
    }

    /// Push a commit to the registry.
    pub async fn push(
        &self,
        owner: &str,
        project: &str,
        commit_json: &serde_json::Value,
        data_files: &HashMap<String, Vec<u8>>,
        transform_files: &HashMap<String, Vec<u8>>,
        lockfiles: &HashMap<String, Vec<u8>>,
    ) -> Result<PushResponse> {
        let mut form = multipart::Form::new();

        // Add commit JSON
        form = form.part(
            "commit",
            multipart::Part::bytes(serde_json::to_vec(commit_json)?)
                .mime_str("application/json")?,
        );

        // Add data files
        for (name, content) in data_files {
            form = form.part(
                format!("data/{}", name),
                multipart::Part::bytes(content.clone()),
            );
        }

        // Add transform files
        for (name, content) in transform_files {
            form = form.part(
                format!("transforms/{}", name),
                multipart::Part::bytes(content.clone()),
            );
        }

        // Add lockfiles
        for (name, content) in lockfiles {
            form = form.part(
                format!("lockfiles/{}", name),
                multipart::Part::bytes(content.clone()),
            );
        }

        let mut request = self
            .client
            .post(format!(
                "{}/api/v1/{}/{}/push",
                self.base_url, owner, project
            ))
            .multipart(form);

        if let Some(auth) = self.auth_header() {
            request = request.header("Authorization", auth);
        }

        let response = request.send().await.context("Failed to push")?;

        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("Push failed: {}", text);
        }

        response
            .json()
            .await
            .context("Failed to parse push response")
    }

    /// Get pull manifest.
    pub async fn pull_manifest(
        &self,
        owner: &str,
        project: &str,
        ref_name: Option<&str>,
    ) -> Result<PullManifest> {
        let mut url = format!(
            "{}/api/v1/{}/{}/pull/manifest",
            self.base_url, owner, project
        );
        if let Some(r) = ref_name {
            url = format!("{}?ref={}", url, r);
        }

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to get pull manifest")?;

        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("Pull manifest failed: {}", text);
        }

        response
            .json()
            .await
            .context("Failed to parse pull manifest")
    }

    /// Pull project as tar archive.
    pub async fn pull(
        &self,
        owner: &str,
        project: &str,
        ref_name: Option<&str>,
    ) -> Result<Vec<u8>> {
        let mut url = format!("{}/api/v1/{}/{}/pull", self.base_url, owner, project);
        if let Some(r) = ref_name {
            url = format!("{}?ref={}", url, r);
        }

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to pull")?;

        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("Pull failed: {}", text);
        }

        response
            .bytes()
            .await
            .map(|b| b.to_vec())
            .context("Failed to read pull response")
    }

    // ========================================================================
    // Endpoint Fetch
    // ========================================================================

    /// Get endpoint manifest.
    pub async fn fetch_manifest(
        &self,
        owner: &str,
        project: &str,
        endpoint: &str,
        ref_name: &str,
    ) -> Result<EndpointManifest> {
        let url = format!(
            "{}/api/v1/{}/{}/{}@{}/manifest",
            self.base_url, owner, project, endpoint, ref_name
        );

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to get endpoint manifest")?;

        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("Fetch manifest failed: {}", text);
        }

        response
            .json()
            .await
            .context("Failed to parse endpoint manifest")
    }

    /// Fetch endpoint content as tar archive.
    pub async fn fetch(
        &self,
        owner: &str,
        project: &str,
        endpoint: &str,
        ref_name: &str,
    ) -> Result<Vec<u8>> {
        let url = format!(
            "{}/api/v1/{}/{}/{}@{}",
            self.base_url, owner, project, endpoint, ref_name
        );

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to fetch endpoint")?;

        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("Fetch failed: {}", text);
        }

        response
            .bytes()
            .await
            .map(|b| b.to_vec())
            .context("Failed to read fetch response")
    }

    /// Resolve endpoint@ref to concrete commit and endpoint metadata.
    pub async fn resolve(
        &self,
        owner: &str,
        project: &str,
        endpoint: &str,
        ref_name: &str,
    ) -> Result<ResolveEndpointResponse> {
        let url = format!(
            "{}/api/v1/resolve/{}/{}/{}@{}",
            self.base_url, owner, project, endpoint, ref_name
        );

        let mut request = self.client.get(&url);
        if let Some(auth) = self.auth_header() {
            request = request.header("Authorization", auth);
        }

        let response = request.send().await.context("Failed to resolve endpoint")?;
        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("Resolve failed: {}", text);
        }

        response
            .json()
            .await
            .context("Failed to parse resolve response")
    }
}
