//! Verification reports and typed witnesses.

mod witness;

pub use witness::{CsvWitness, RecordWitness, TableColumnWitness, TableWitness};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerificationVerdict {
    Verified,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VerificationReport {
    pub verifier: String,
    pub verdict: VerificationVerdict,
    #[serde(default)]
    pub diagnostics: Vec<String>,
    #[serde(default)]
    pub evidence: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verification_report_can_carry_diagnostics() {
        let report = VerificationReport {
            verifier: "builtin.csv".to_string(),
            verdict: VerificationVerdict::Verified,
            diagnostics: vec!["parsed successfully".to_string()],
            evidence: None,
        };

        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(report.verdict, VerificationVerdict::Verified);
    }
}
