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
