//! Types for the compute backend.

use std::collections::HashMap;
use std::path::PathBuf;

/// A request to execute a transform in a container.
#[derive(Debug, Clone)]
pub struct ComputeRequest {
    /// Docker image to run (e.g., "ozzydb-env:abc123").
    pub image: String,
    /// Generated runner script content.
    pub runner_script: String,
    /// Runner file extension ("py", "R", "sh").
    pub runner_ext: String,
    /// Generated init script content.
    pub init_script: String,
    /// Input files to mount into the container.
    pub inputs: Vec<InputSpec>,
    /// Environment variables to set in the container.
    pub env_vars: HashMap<String, String>,
    /// Timeout in seconds.
    pub timeout_secs: u64,
    /// Memory limit (e.g., "2g").
    pub memory_limit: Option<String>,
    /// CPU limit (e.g., "1").
    pub cpu_limit: Option<String>,
    /// Whether the container needs network access.
    pub network: bool,
    /// Docker runtime (e.g., "runsc" for gVisor).
    pub runtime: Option<String>,
    /// Source directory to mount (for local execution).
    pub source_dir: Option<PathBuf>,
}

/// Specification for an input file to mount into the container.
#[derive(Debug, Clone)]
pub struct InputSpec {
    /// Input name (matches the key in the input manifest).
    pub name: String,
    /// Local path to the input file.
    pub local_path: PathBuf,
    /// Content type of the input.
    pub content_type: String,
    /// Whether this input is a collection directory.
    pub is_collection: bool,
}

/// Result of a compute execution.
#[derive(Debug)]
pub struct ComputeResult {
    /// Path to the output directory (contains the transform's output files).
    pub output_dir: PathBuf,
    /// Path to the workspace directory (for cleanup after reading output).
    pub workspace_dir: PathBuf,
    /// Container exit code.
    pub exit_code: i32,
    /// Combined stdout + stderr from the container.
    pub logs: String,
    /// Execution duration in milliseconds.
    pub duration_ms: u64,
}

impl ComputeResult {
    /// Whether the execution succeeded (exit code 0).
    pub fn success(&self) -> bool {
        self.exit_code == 0
    }

    /// Clean up the workspace directory (call after reading output).
    pub async fn cleanup(self) {
        crate::compute::docker::cleanup_workspace(self.workspace_dir).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_result_success() {
        let result = ComputeResult {
            output_dir: PathBuf::from("/tmp/output"),
            workspace_dir: PathBuf::from("/tmp/workspace"),
            exit_code: 0,
            logs: "OK".to_string(),
            duration_ms: 100,
        };
        assert!(result.success());
    }

    #[test]
    fn test_compute_result_failure() {
        let result = ComputeResult {
            output_dir: PathBuf::from("/tmp/output"),
            workspace_dir: PathBuf::from("/tmp/workspace"),
            exit_code: 1,
            logs: "Error".to_string(),
            duration_ms: 50,
        };
        assert!(!result.success());
    }
}
