//! Database models matching PostgreSQL schema.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// User record.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub email: Option<String>,
    pub github_id: Option<i64>,
    pub github_login: Option<String>,
    pub avatar_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Project record.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Project {
    pub id: Uuid,
    pub owner_user_id: Uuid,
    pub slug: String,
    pub description: Option<String>,
    pub visibility: String,
    pub default_branch: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Commit record (metadata only, content in storage).
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct DbCommit {
    pub id: Uuid,
    pub project_id: Uuid,
    pub hash: String,
    pub parent_hashes: Vec<String>,
    pub author_id: Option<Uuid>,
    pub author_name: String,
    pub message: String,
    pub commit_timestamp: String,
    pub created_at: DateTime<Utc>,
}

/// Data source record.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct DbDataSource {
    pub id: Uuid,
    pub commit_id: Uuid,
    pub name: String,
    pub content_hash: String,
    pub schema_hash: String,
    pub storage_key: String,
    pub schema_json: serde_json::Value,
    pub row_count: Option<i64>,
    pub byte_size: Option<i64>,
}

/// Transform record.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct DbTransform {
    pub id: Uuid,
    pub commit_id: Uuid,
    pub name: String,
    pub content_hash: String,
    pub runtime: String,
    pub source_storage_key: String,
    pub function_name: String,
    pub lockfile_hash: String,
    pub params_schema: serde_json::Value,
    pub input_schema: Option<serde_json::Value>,
    pub output_schema: Option<serde_json::Value>,
    pub reproducible: bool,
}

/// Endpoint record.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct DbEndpoint {
    pub id: Uuid,
    pub commit_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub definition: serde_json::Value,
}

/// Ref record (branch or tag).
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct DbRef {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub ref_type: String,
    pub commit_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Project collaborator record.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ProjectCollaborator {
    pub project_id: Uuid,
    pub user_id: Uuid,
    pub permission: String,
    pub created_at: DateTime<Utc>,
}

/// Project collaborator joined with username.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ProjectCollaboratorWithUser {
    pub user_id: Uuid,
    pub username: String,
    pub permission: String,
    pub created_at: DateTime<Utc>,
}

/// API token record.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ApiToken {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub token_hash: String,
    pub token_prefix: String,
    pub scopes: Vec<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Content reference (for deduplication tracking).
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ContentRef {
    pub content_hash: String,
    pub storage_key: String,
    pub content_type: String,
    pub byte_size: i64,
    pub ref_count: i32,
    pub created_at: DateTime<Utc>,
}
