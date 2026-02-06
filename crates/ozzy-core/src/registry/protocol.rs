//! Registry protocol types for API communication.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// Note: We use simpler types in protocol responses rather than full project types
// to decouple the wire format from internal representations.

// ============================================================================
// Authentication
// ============================================================================

/// GitHub device flow initiation response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

/// GitHub device flow poll request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCodePollRequest {
    pub device_code: String,
}

/// Successful authentication response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthResponse {
    pub access_token: Option<String>,
    pub token_type: Option<String>,
    pub user: Option<UserInfo>,
    #[serde(default)]
    pub pending: bool,
}

/// User information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub id: Uuid,
    pub username: String,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
}

/// API token creation request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTokenRequest {
    pub name: String,
    pub scopes: Vec<String>,
    pub expires_in_days: Option<u32>,
}

/// API token creation response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTokenResponse {
    pub id: Uuid,
    pub name: String,
    pub token: String, // Only returned on creation
    pub scopes: Vec<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// API token listing item (without the actual token).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenInfo {
    pub id: Uuid,
    pub name: String,
    pub scopes: Vec<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

// ============================================================================
// Project Operations
// ============================================================================

/// Project metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectInfo {
    pub id: Uuid,
    pub owner: String,
    pub slug: String,
    pub description: Option<String>,
    pub visibility: String,
    pub default_branch: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Create project request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProjectRequest {
    pub slug: String,
    pub description: Option<String>,
    pub visibility: Option<String>,
}

/// Add/update collaborator request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddCollaboratorRequest {
    pub username: String,
    pub permission: String, // read|write|admin
}

/// Collaborator entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollaboratorInfo {
    pub username: String,
    pub permission: String,
    pub added_at: DateTime<Utc>,
}

/// Ref information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefInfo {
    pub name: String,
    pub ref_type: String, // "branch" or "tag"
    pub commit_hash: String,
    pub updated_at: String,
}

/// List refs response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListRefsResponse {
    pub branches: Vec<RefInfo>,
    pub tags: Vec<RefInfo>,
}

/// Commit information returned by commit-history APIs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitInfo {
    pub hash: String,
    pub parent_hashes: Vec<String>,
    pub author: String,
    pub message: String,
    pub created_at: DateTime<Utc>,
}

// ============================================================================
// Push/Pull Operations
// ============================================================================

/// Request to check which content already exists on server (for deduplication).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentCheckRequest {
    pub data_hashes: HashMap<String, String>,
    pub transform_hashes: HashMap<String, String>,
}

/// Response indicating which content is missing on server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentCheckResponse {
    pub missing_data: Vec<String>,
    pub missing_transforms: Vec<String>,
}

/// Push response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushResponse {
    pub commit_hash: String,
    pub ref_name: String,
    pub new_data_count: usize,
    pub new_transform_count: usize,
}

/// Pull manifest - metadata about what's available to download.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullManifest {
    pub commit_hash: String,
    pub commit_message: String,
    pub data_hashes: HashMap<String, String>,
    pub transform_hashes: HashMap<String, String>,
    pub total_size_bytes: u64,
}

/// Content download request (for selective pull).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentDownloadRequest {
    pub data: Vec<String>,
    pub transforms: Vec<String>,
}

// ============================================================================
// Endpoint Fetch Operations
// ============================================================================

/// Endpoint manifest for remote fetch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointManifest {
    pub commit_hash: String,
    pub endpoint: serde_json::Value,
    pub data_hashes: HashMap<String, String>,
    pub transform_hashes: HashMap<String, String>,
    pub total_size_bytes: u64,
}

/// Resolve response for endpoint@ref references.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveEndpointResponse {
    pub owner: String,
    pub project: String,
    pub endpoint: String,
    pub ref_name: String,
    pub commit_hash: String,
    pub definition: serde_json::Value,
}

// ============================================================================
// Error Responses
// ============================================================================

/// API error response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    pub error: String,
    pub message: String,
    pub details: Option<serde_json::Value>,
}

impl ApiError {
    pub fn new(error: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            message: message.into(),
            details: None,
        }
    }

    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }
}

// ============================================================================
// Credential Storage
// ============================================================================

/// Stored credentials for a registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryCredentials {
    pub access_token: String,
    pub username: String,
    pub expires_at: Option<String>,
}

/// Credentials file structure.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CredentialsFile {
    pub default: Option<String>,
    pub registries: HashMap<String, RegistryCredentials>,
}

impl CredentialsFile {
    /// Get credentials for a registry URL.
    pub fn get(&self, registry_url: &str) -> Option<&RegistryCredentials> {
        self.registries.get(registry_url)
    }

    /// Set credentials for a registry URL.
    pub fn set(&mut self, registry_url: String, credentials: RegistryCredentials) {
        self.registries.insert(registry_url, credentials);
    }

    /// Remove credentials for a registry URL.
    pub fn remove(&mut self, registry_url: &str) -> Option<RegistryCredentials> {
        self.registries.remove(registry_url)
    }

    /// Get the default registry URL.
    pub fn default_registry(&self) -> Option<&str> {
        self.default.as_deref()
    }
}

// ============================================================================
// Remote Configuration
// ============================================================================

/// Remote registry configuration stored in ozzy.toml.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RemotesConfig {
    #[serde(flatten)]
    pub remotes: HashMap<String, String>,
}

impl RemotesConfig {
    /// Get the URL for a named remote.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.remotes.get(name).map(|s| s.as_str())
    }

    /// Add or update a remote.
    pub fn set(&mut self, name: String, url: String) {
        self.remotes.insert(name, url);
    }

    /// Remove a remote.
    pub fn remove(&mut self, name: &str) -> Option<String> {
        self.remotes.remove(name)
    }

    /// List all remote names.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.remotes.keys().map(|s| s.as_str())
    }
}
