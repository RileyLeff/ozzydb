//! Database models matching the v2 PostgreSQL schema.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

// ============================================================
// Users
// ============================================================

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct User {
    pub id: Uuid,
    pub github_id: Option<i64>,
    pub username: String,
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    pub is_admin: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ============================================================
// API tokens
// ============================================================

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ApiToken {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token_hash: String,
    pub name: String,
    pub scope: String,
    pub project_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
}

// ============================================================
// Projects
// ============================================================

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Project {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub slug: String,
    pub description: Option<String>,
    pub visibility: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ProjectCollaborator {
    pub project_id: Uuid,
    pub user_id: Uuid,
    pub role: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ProjectCollaboratorWithUser {
    pub user_id: Uuid,
    pub username: String,
    pub role: String,
    pub created_at: DateTime<Utc>,
}

// ============================================================
// Commits
// ============================================================

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Commit {
    pub id: Uuid,
    pub project_id: Uuid,
    pub git_provider: String,
    pub git_repo: String,
    pub git_commit_sha: String,
    pub ozzy_toml_hash: String,
    pub pushed_by: Uuid,
    pub message: Option<String>,
    pub created_at: DateTime<Utc>,
}

// ============================================================
// Refs
// ============================================================

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Ref {
    pub id: Uuid,
    pub project_id: Uuid,
    pub ref_name: String,
    pub ref_type: String,
    pub commit_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ContentRef {
    pub hash: String,
    pub r2_key: String,
    pub content_type: String,
    pub byte_size: i64,
    pub ref_count: i32,
    pub created_at: DateTime<Utc>,
}

// ============================================================
// Secrets
// ============================================================

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Secret {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    #[serde(skip_serializing)]
    pub encrypted_value: Vec<u8>,
    pub version_id: Uuid,
    pub set_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Secret metadata (without the encrypted value — for listing).
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct SecretInfo {
    pub id: Uuid,
    pub name: String,
    pub version_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ============================================================
// Environment images
// ============================================================

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct EnvironmentImage {
    pub id: Uuid,
    pub env_hash: String,
    pub image_ref: String,
    pub build_type: String,
    pub base_image: Option<String>,
    pub build_log_r2_key: Option<String>,
    pub built_at: Option<DateTime<Utc>>,
    pub build_duration_ms: Option<i32>,
    pub created_at: DateTime<Utc>,
}

// ============================================================
// Source cache
// ============================================================

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct SourceCacheEntry {
    pub id: Uuid,
    pub git_provider: String,
    pub git_repo: String,
    pub git_commit_sha: String,
    pub r2_key: String,
    pub byte_size: i64,
    pub cached_at: DateTime<Utc>,
    pub last_accessed: DateTime<Utc>,
}

// ============================================================
// Materialized cache
// ============================================================

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct MaterializedCacheEntry {
    pub materialized_hash: String,
    pub project_id: Uuid,
    pub project_revision_id: Uuid,
    pub endpoint_name: String,
    pub node_name: String,
    pub transform_version_id: Uuid,
    pub environment_version_id: Uuid,
    pub params_hash: String,
    pub input_artifact_bindings: serde_json::Value,
    pub source_hash: String,
    pub secrets_hash: Option<String>,
    pub output_artifact_id: Uuid,
    pub output_hash: String,
    pub output_r2_key: String,
    pub output_content_type: String,
    pub output_byte_size: i64,
    pub computed_at: DateTime<Utc>,
    pub last_accessed: DateTime<Utc>,
    pub access_count: i32,
}

// ============================================================
// Jobs (async compute)
// ============================================================

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Job {
    pub id: Uuid,
    pub project_id: Uuid,
    pub endpoint_name: String,
    pub commit_id: Uuid,
    pub params: serde_json::Value,
    pub params_hash: String,
    pub input_bindings: serde_json::Value,
    pub input_bindings_hash: String,
    pub status: String,
    pub node_status: serde_json::Value,
    pub output_hash: Option<String>,
    pub output_content_type: Option<String>,
    pub error_message: Option<String>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
}

// ============================================================
// Environment provider images
// ============================================================

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct EnvironmentProviderImage {
    pub id: Uuid,
    pub env_hash: String,
    pub provider: String,
    pub image_ref: String,
    pub pushed_at: DateTime<Utc>,
}

// ============================================================
// GitHub installations
// ============================================================

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct GitHubInstallation {
    pub id: Uuid,
    pub installation_id: i64,
    pub account_type: String,
    pub account_login: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
