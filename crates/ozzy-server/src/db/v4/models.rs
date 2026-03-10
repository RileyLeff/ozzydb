//! Database models for the v4 registry object model.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct StoredCanonicalType {
    pub id: Uuid,
    pub canonical_key: String,
    pub expr: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct StoredTypeVersion {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub version: String,
    pub canonical_type_id: Uuid,
    pub expr: Value,
    pub published_by: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct StoredEnvironmentVersion {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub version: String,
    pub definition: Value,
    pub published_by: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct StoredTransformVersion {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub version: String,
    pub environment_version_id: Uuid,
    pub source_ref: Option<String>,
    pub command: Option<String>,
    pub description: Option<String>,
    pub params_schema: Value,
    pub network_access: bool,
    pub secrets: Vec<String>,
    pub published_by: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransformPortKind {
    Input,
    Output,
}

impl TransformPortKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Output => "output",
        }
    }
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct StoredTransformPort {
    pub id: Uuid,
    pub transform_version_id: Uuid,
    pub port_kind: String,
    pub port_name: String,
    pub type_version_id: Uuid,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct StoredRegistryRevision {
    pub id: Uuid,
    pub project_id: Uuid,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct StoredRegistryRevisionTypeVersion {
    pub registry_revision_id: Uuid,
    pub type_version_id: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct StoredRegistryRevisionEnvironmentVersion {
    pub registry_revision_id: Uuid,
    pub environment_version_id: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct StoredRegistryRevisionTransformVersion {
    pub registry_revision_id: Uuid,
    pub transform_version_id: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct StoredProjectRevision {
    pub id: Uuid,
    pub project_id: Uuid,
    pub source_commit_id: Uuid,
    pub registry_revision_id: Uuid,
    pub ozzy_toml_hash: String,
    pub ozzy_toml_raw: String,
    pub environments: Value,
    pub transforms: Value,
    pub endpoints: Value,
    pub project_meta: Value,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct StoredInvocation {
    pub id: Uuid,
    pub project_revision_id: Uuid,
    pub transform_version_id: Uuid,
    pub endpoint_name: Option<String>,
    pub node_name: Option<String>,
    pub params: Value,
    pub params_hash: String,
    pub input_bindings: Value,
    pub output_bindings: Value,
    pub status: String,
    pub error_message: Option<String>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct StoredConformanceRecord {
    pub id: Uuid,
    pub artifact_id: Uuid,
    pub type_version_id: Uuid,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct StoredVerificationAttempt {
    pub id: Uuid,
    pub conformance_record_id: Uuid,
    pub verifier: String,
    pub attempt_kind: String,
    pub verdict: Option<String>,
    pub diagnostics: Value,
    pub evidence: Option<Value>,
    pub failure_error: Option<String>,
    pub created_at: DateTime<Utc>,
}
