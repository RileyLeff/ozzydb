//! Artifact-to-type conformance records.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::registry::TypeVersionId;
use crate::verify::{VerificationReport, VerificationVerdict};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConformanceStatus {
    Declared,
    Verified,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerificationFailure {
    pub verifier: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VerificationAttempt {
    Completed { report: VerificationReport },
    Failed(VerificationFailure),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConformanceRecord {
    pub artifact_id: String,
    pub type_version: TypeVersionId,
    pub status: ConformanceStatus,
    #[serde(default)]
    pub attempts: Vec<VerificationAttempt>,
}

impl ConformanceRecord {
    pub fn declared(artifact_id: impl Into<String>, type_version: TypeVersionId) -> Self {
        Self {
            artifact_id: artifact_id.into(),
            type_version,
            status: ConformanceStatus::Declared,
            attempts: Vec::new(),
        }
    }

    pub fn record_report(&mut self, report: VerificationReport) {
        self.status = match report.verdict {
            VerificationVerdict::Verified => ConformanceStatus::Verified,
            VerificationVerdict::Rejected => ConformanceStatus::Rejected,
        };
        self.attempts
            .push(VerificationAttempt::Completed { report });
    }

    pub fn record_failure(&mut self, failure: VerificationFailure) {
        self.attempts.push(VerificationAttempt::Failed(failure));
    }

    pub fn latest_report(&self) -> Option<&VerificationReport> {
        self.attempts
            .iter()
            .rev()
            .find_map(|attempt| match attempt {
                VerificationAttempt::Completed { report } => Some(report),
                VerificationAttempt::Failed(_) => None,
            })
    }

    pub fn latest_evidence(&self) -> Option<&Value> {
        self.latest_report()
            .and_then(|report| report.evidence.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn conformance_status_serializes_as_snake_case() {
        let value = serde_json::to_value(ConformanceStatus::Verified).expect("serialize status");
        assert_eq!(value, serde_json::Value::String("verified".to_string()));
    }

    #[test]
    fn declared_record_starts_without_attempts() {
        let record = ConformanceRecord::declared("artifact_123", TypeVersionId::new("std/T@1"));
        assert_eq!(record.status, ConformanceStatus::Declared);
        assert!(record.attempts.is_empty());
        assert!(record.latest_report().is_none());
        assert!(record.latest_evidence().is_none());
    }

    #[test]
    fn completed_verification_updates_status_and_evidence() {
        let mut record = ConformanceRecord::declared("artifact_123", TypeVersionId::new("std/T@1"));
        record.record_report(VerificationReport {
            verifier: "builtin.v1".to_string(),
            verdict: VerificationVerdict::Verified,
            diagnostics: Vec::new(),
            evidence: Some(json!({ "kind": "table" })),
        });

        assert_eq!(record.status, ConformanceStatus::Verified);
        assert_eq!(record.attempts.len(), 1);
        assert_eq!(record.latest_evidence(), Some(&json!({ "kind": "table" })));
    }

    #[test]
    fn failed_attempt_does_not_change_semantic_status() {
        let mut record = ConformanceRecord::declared("artifact_123", TypeVersionId::new("std/T@1"));
        record.record_failure(VerificationFailure {
            verifier: "builtin.v1".to_string(),
            error: "missing parquet codec".to_string(),
        });

        assert_eq!(record.status, ConformanceStatus::Declared);
        assert_eq!(record.attempts.len(), 1);
        assert!(record.latest_report().is_none());
    }

    #[test]
    fn rejected_report_sets_rejected_status() {
        let mut record = ConformanceRecord::declared("artifact_123", TypeVersionId::new("std/T@1"));
        record.record_report(VerificationReport {
            verifier: "builtin.v1".to_string(),
            verdict: VerificationVerdict::Rejected,
            diagnostics: vec!["missing required column 'wp'".to_string()],
            evidence: Some(json!({ "kind": "record" })),
        });

        assert_eq!(record.status, ConformanceStatus::Rejected);
        assert_eq!(
            record.latest_report().unwrap().verdict,
            VerificationVerdict::Rejected
        );
    }
}
