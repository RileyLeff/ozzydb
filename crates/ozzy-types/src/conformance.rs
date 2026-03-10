//! Artifact-to-type conformance records.

use serde::{Deserialize, Serialize};

use crate::registry::TypeVersionId;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConformanceStatus {
    Declared,
    Verified,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConformanceRecord {
    pub artifact_id: String,
    pub type_version: TypeVersionId,
    pub status: ConformanceStatus,
    #[serde(default)]
    pub evidence: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conformance_status_serializes_as_snake_case() {
        let value = serde_json::to_value(ConformanceStatus::Verified).expect("serialize status");
        assert_eq!(value, serde_json::Value::String("verified".to_string()));
    }
}
