//! Types for the compute backend.

use std::collections::HashMap;

use anyhow::Result;

/// Trait for compute backends (Docker, Fly Machines, etc.).
///
/// Each backend creates a container with the given image and env vars,
/// waits for completion, and returns exit code + logs. All I/O (inputs,
/// output, source code, secrets) is handled via presigned URLs encoded
/// in env_vars by the orchestrator.
pub trait ComputeBackend: Send + Sync {
    /// Execute a transform in a container and return the result.
    fn run(
        &self,
        request: &ComputeRequest,
    ) -> impl std::future::Future<Output = Result<ComputeResult>> + Send;
}

/// A request to execute a transform in a container.
///
/// All I/O is encoded in `env_vars` as presigned URLs:
/// - `OZZY_INIT_SCRIPT_B64`: base64-encoded init script
/// - `OZZY_RUNNER_SCRIPT_B64`: base64-encoded runner script
/// - `OZZY_INPUT_DOWNLOADS`: JSON array of input download specs
/// - `OZZY_SOURCE_DOWNLOAD`: presigned GET URL for source tarball
/// - `OZZY_OUTPUT_UPLOAD_URL`: presigned PUT URL for output tarball
/// - `OZZY_SECRETS_URL`: presigned GET URL for secrets JSON
/// - `OZZY_INPUT_MANIFEST`: JSON manifest of inputs
/// - `OZZY_PARAMS`: JSON params object
/// - `OZZY_PARAM_*`: individual param values
/// - `PYTHONHASHSEED`, `OMP_NUM_THREADS`, etc.: determinism env vars
#[derive(Debug, Clone)]
pub struct ComputeRequest {
    /// Docker image to run (e.g., "ozzydb-env:abc123").
    pub image: String,
    /// Environment variables to set in the container.
    /// Contains all OZZY_* vars, determinism vars, params, etc.
    pub env_vars: HashMap<String, String>,
    /// Timeout in seconds.
    pub timeout_secs: u64,
    /// Memory limit (e.g., "2g").
    pub memory_limit: Option<String>,
    /// CPU limit (e.g., "1").
    pub cpu_limit: Option<String>,
}

/// Specification for an input file (used to build the input manifest).
#[derive(Debug, Clone)]
pub struct InputSpec {
    /// Input name (matches the key in the input manifest).
    pub name: String,
    /// Content type of the input.
    pub content_type: String,
    /// Whether this input is a collection directory.
    pub is_collection: bool,
}

/// Result of a compute execution.
#[derive(Debug)]
pub struct ComputeResult {
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
}

/// Build the input manifest JSON for a set of inputs.
///
/// This produces the JSON blob that the runner reads from `OZZY_INPUT_MANIFEST`.
pub fn build_input_manifest(inputs: &[InputSpec]) -> serde_json::Value {
    let mut manifest = serde_json::Map::new();
    for input in inputs {
        let container_path = format!("/workspace/inputs/{}", input.name);
        let mut spec = serde_json::Map::new();
        spec.insert("path".to_string(), serde_json::json!(container_path));
        spec.insert(
            "content_type".to_string(),
            serde_json::json!(input.content_type),
        );
        spec.insert(
            "is_collection".to_string(),
            serde_json::json!(input.is_collection),
        );
        if input.is_collection {
            spec.insert(
                "manifest_path".to_string(),
                serde_json::json!(format!("{}/manifest.json", container_path)),
            );
        }
        manifest.insert(input.name.clone(), serde_json::Value::Object(spec));
    }
    serde_json::Value::Object(manifest)
}

/// Build the per-param env vars (OZZY_PARAM_*).
///
/// Keys are sanitized to `[a-zA-Z0-9_]` — any non-matching characters are stripped.
pub fn build_param_env_vars(params: &serde_json::Value) -> Vec<(String, String)> {
    let mut vars = Vec::new();
    if let Some(obj) = params.as_object() {
        for (key, value) in obj {
            let sanitized: String = key
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if sanitized.is_empty() {
                continue;
            }
            let str_value = match value {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            vars.push((format!("OZZY_PARAM_{}", sanitized), str_value));
        }
    }
    vars
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_result_success() {
        let result = ComputeResult {
            exit_code: 0,
            logs: "OK".to_string(),
            duration_ms: 100,
        };
        assert!(result.success());
    }

    #[test]
    fn test_compute_result_failure() {
        let result = ComputeResult {
            exit_code: 1,
            logs: "Error".to_string(),
            duration_ms: 50,
        };
        assert!(!result.success());
    }

    #[test]
    fn test_build_input_manifest_single() {
        let inputs = vec![InputSpec {
            name: "readings".to_string(),
            content_type: "application/vnd.apache.parquet".to_string(),
            is_collection: false,
        }];

        let manifest = build_input_manifest(&inputs);
        let readings = manifest.get("readings").unwrap();
        assert_eq!(readings.get("path").unwrap(), "/workspace/inputs/readings");
        assert_eq!(
            readings.get("content_type").unwrap(),
            "application/vnd.apache.parquet"
        );
        assert_eq!(readings.get("is_collection").unwrap(), false);
    }

    #[test]
    fn test_build_input_manifest_collection() {
        let inputs = vec![InputSpec {
            name: "all_readings".to_string(),
            content_type: "collection".to_string(),
            is_collection: true,
        }];

        let manifest = build_input_manifest(&inputs);
        let coll = manifest.get("all_readings").unwrap();
        assert_eq!(coll.get("is_collection").unwrap(), true);
        assert!(coll.get("manifest_path").is_some());
    }

    #[test]
    fn test_build_param_env_vars() {
        let params = serde_json::json!({
            "threshold": 12.5,
            "format": "csv",
            "debug": true,
        });

        let vars = build_param_env_vars(&params);
        assert_eq!(vars.len(), 3);

        let var_map: std::collections::HashMap<_, _> = vars.into_iter().collect();
        assert_eq!(var_map.get("OZZY_PARAM_threshold").unwrap(), "12.5");
        assert_eq!(var_map.get("OZZY_PARAM_format").unwrap(), "csv");
        assert_eq!(var_map.get("OZZY_PARAM_debug").unwrap(), "true");
    }

    #[test]
    fn test_build_param_env_vars_empty() {
        let params = serde_json::json!({});
        let vars = build_param_env_vars(&params);
        assert!(vars.is_empty());
    }

    #[test]
    fn test_build_param_env_vars_string_not_quoted() {
        let params = serde_json::json!({ "name": "alice" });
        let vars = build_param_env_vars(&params);
        assert_eq!(vars[0].1, "alice");
    }
}
