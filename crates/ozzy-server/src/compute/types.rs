//! Types for the compute backend.

use std::collections::{BTreeMap, HashMap};

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Trait for compute backends (Docker, Fly Machines, etc.).
///
/// Each backend creates a container with the given image and env vars,
/// waits for completion, and returns exit code + logs. All I/O (inputs,
/// output, source code, secrets) is handled via presigned URLs encoded
/// in env_vars by the orchestrator.
pub trait ComputeBackend: Send + Sync + std::any::Any {
    /// Execute a transform in a container and return the result.
    fn run<'a>(
        &'a self,
        request: &'a ComputeRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ComputeResult>> + Send + 'a>>;
}

/// A request to execute a transform in a container.
#[derive(Debug, Clone)]
pub struct ComputeRequest {
    pub image: String,
    pub env_vars: HashMap<String, String>,
    pub timeout_secs: u64,
    pub network_enabled: bool,
}

/// Loader hint for a blob-like input artifact.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InputLoader {
    Bytes,
    Csv,
    Json,
    Parquet,
    Text,
}

/// A manifest entry for one blob-like input file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InputBlobSpec {
    pub path: String,
    pub loader: InputLoader,
}

/// Runtime input manifest shape.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InputSpec {
    Blob {
        path: String,
        loader: InputLoader,
    },
    Bundle {
        entries: BTreeMap<String, InputSpec>,
    },
    Collection {
        items: Vec<InputSpec>,
    },
}

/// Result of a compute execution.
#[derive(Debug)]
pub struct ComputeResult {
    pub exit_code: i32,
    pub logs: String,
    pub duration_ms: u64,
}

impl ComputeResult {
    pub fn success(&self) -> bool {
        self.exit_code == 0
    }
}

/// Build the input manifest JSON for a set of named inputs.
pub fn build_input_manifest(inputs: &BTreeMap<String, InputSpec>) -> serde_json::Value {
    serde_json::to_value(inputs).expect("input manifest serialization should be infallible")
}

/// Build the per-param env vars (OZZY_PARAM_*).
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
    fn test_build_input_manifest_blob() {
        let manifest = build_input_manifest(&BTreeMap::from([(
            "readings".to_string(),
            InputSpec::Blob {
                path: "/workspace/inputs/readings".to_string(),
                loader: InputLoader::Parquet,
            },
        )]));

        let readings = manifest.get("readings").unwrap();
        assert_eq!(readings.get("kind").unwrap(), "blob");
        assert_eq!(readings.get("path").unwrap(), "/workspace/inputs/readings");
        assert_eq!(readings.get("loader").unwrap(), "parquet");
    }

    #[test]
    fn test_build_input_manifest_collection() {
        let manifest = build_input_manifest(&BTreeMap::from([(
            "all_readings".to_string(),
            InputSpec::Collection {
                items: vec![
                    InputSpec::Blob {
                        path: "/workspace/inputs/all_readings/item_0".to_string(),
                        loader: InputLoader::Parquet,
                    },
                    InputSpec::Blob {
                        path: "/workspace/inputs/all_readings/item_1".to_string(),
                        loader: InputLoader::Parquet,
                    },
                ],
            },
        )]));

        let coll = manifest.get("all_readings").unwrap();
        assert_eq!(coll.get("kind").unwrap(), "collection");
        assert_eq!(coll.get("items").unwrap().as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_build_input_manifest_bundle() {
        let manifest = build_input_manifest(&BTreeMap::from([(
            "bundle".to_string(),
            InputSpec::Bundle {
                entries: BTreeMap::from([
                    (
                        "obs".to_string(),
                        InputSpec::Blob {
                            path: "/workspace/inputs/bundle/obs".to_string(),
                            loader: InputLoader::Parquet,
                        },
                    ),
                    (
                        "meta".to_string(),
                        InputSpec::Blob {
                            path: "/workspace/inputs/bundle/meta".to_string(),
                            loader: InputLoader::Json,
                        },
                    ),
                ]),
            },
        )]));

        let bundle = manifest.get("bundle").unwrap();
        assert_eq!(bundle.get("kind").unwrap(), "bundle");
        let entries = bundle.get("entries").unwrap().as_object().unwrap();
        assert_eq!(entries["obs"]["loader"], "parquet");
        assert_eq!(entries["meta"]["loader"], "json");
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
}
