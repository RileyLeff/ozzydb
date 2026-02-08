//! Single-transform Docker execution.
//!
//! Runs a transform in a sandboxed container with `--network=none`,
//! `--read-only`, resource limits, and deterministic env vars.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tokio::process::Command;

use crate::config::ComputeConfig;

/// Generate the Python runner script for a container execution.
///
/// All paths are container-absolute (`/in/`, `/out/`, `/work/`).
pub fn generate_run_script(
    function_name: &str,
    inputs: &HashMap<String, PathBuf>,
    params: &serde_json::Value,
) -> Result<String> {
    // Sort inputs by name for determinism
    let mut sorted_inputs: Vec<_> = inputs.keys().collect();
    sorted_inputs.sort();

    let input_lines: String = sorted_inputs
        .iter()
        .map(|name| {
            format!(
                "inputs[\"{}\"] = pl.read_parquet(\"/in/{}.parquet\")",
                escape_python_string(name),
                escape_python_string(name),
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let params_json = serde_json::to_string(params)?;
    let escaped_params = escape_python_string(&params_json);

    Ok(format!(
        r#"import sys
import json
import polars as pl

sys.path.insert(0, "/work")
import transform

inputs = {{}}
{input_lines}

params_dict = json.loads("{escaped_params}")

class Params:
    def __init__(self, d):
        for k, v in d.items():
            setattr(self, k, v)
    def get(self, key, default=None):
        return getattr(self, key, default)

params = Params(params_dict)
result = transform.{function_name}(inputs, params)

if hasattr(result, 'collect'):
    result = result.collect()

result.write_parquet("/out/output.parquet")
"#,
        input_lines = input_lines,
        escaped_params = escaped_params,
        function_name = function_name,
    ))
}

/// Execute a single transform in a Docker container.
///
/// - `image_tag`: Docker image to use (e.g., `ozzy-env:base`)
/// - `transform_source`: Python source code bytes
/// - `function_name`: function to call in the transform module
/// - `input_dir`: host directory containing `{name}.parquet` files
/// - `output_dir`: host directory where `output.parquet` will be written
/// - `params`: JSON params for the transform
pub async fn execute_transform(
    config: &ComputeConfig,
    image_tag: &str,
    transform_source: &[u8],
    function_name: &str,
    input_dir: &Path,
    output_dir: &Path,
    params: &serde_json::Value,
) -> Result<()> {
    // Create a work directory with the transform source and run script
    let work_dir = tempfile::tempdir().context("Failed to create work dir")?;

    tokio::fs::write(work_dir.path().join("transform.py"), transform_source)
        .await
        .context("Failed to write transform.py")?;

    // Build input map (just names — paths are fixed in container)
    let input_names: HashMap<String, PathBuf> = std::fs::read_dir(input_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .map(|ext| ext == "parquet")
                .unwrap_or(false)
        })
        .filter_map(|e| {
            e.path()
                .file_stem()
                .map(|s| (s.to_string_lossy().to_string(), e.path()))
        })
        .collect();

    let run_script = generate_run_script(function_name, &input_names, params)?;
    tokio::fs::write(work_dir.path().join("run.py"), &run_script)
        .await
        .context("Failed to write run.py")?;

    // Build docker run command
    let mut cmd = Command::new("docker");
    cmd.args(["run", "--rm"]);

    // Runtime (gVisor or plain Docker)
    cmd.args(["--runtime", &config.docker_runtime]);

    // Security constraints — always enforced regardless of runtime
    cmd.args(["--network", "none"]);
    cmd.arg("--read-only");

    // Resource limits
    cmd.args(["--memory", &config.memory_limit]);
    cmd.args(["--cpus", &config.cpu_limit]);
    cmd.args([
        "--tmpfs",
        &format!("/tmp:size={}", config.tmpfs_size),
    ]);

    // Volume mounts
    cmd.args([
        "-v",
        &format!("{}:/in:ro", input_dir.display()),
    ]);
    cmd.args([
        "-v",
        &format!("{}:/out", output_dir.display()),
    ]);
    cmd.args([
        "-v",
        &format!("{}:/work:ro", work_dir.path().display()),
    ]);

    // Deterministic env vars
    cmd.args(["-e", "PYTHONHASHSEED=0"]);
    cmd.args(["-e", "OMP_NUM_THREADS=1"]);
    cmd.args(["-e", "MKL_NUM_THREADS=1"]);
    cmd.args(["-e", "OPENBLAS_NUM_THREADS=1"]);
    cmd.args(["-e", "NUMEXPR_NUM_THREADS=1"]);

    // Image and command
    cmd.arg(image_tag);
    cmd.args(["python", "/work/run.py"]);

    // Execute with timeout
    let timeout = std::time::Duration::from_secs(config.timeout_secs);
    let output = tokio::time::timeout(timeout, cmd.output())
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "Transform execution timed out after {}s",
                config.timeout_secs
            )
        })?
        .context("Failed to execute docker run")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        anyhow::bail!(
            "Transform execution failed (exit {}):\nstdout: {}\nstderr: {}",
            output.status.code().unwrap_or(-1),
            stdout,
            stderr,
        );
    }

    // Verify output exists
    let output_parquet = output_dir.join("output.parquet");
    if !output_parquet.exists() {
        anyhow::bail!(
            "Transform produced no output (expected {})",
            output_parquet.display()
        );
    }

    Ok(())
}

/// Escape a string for safe embedding in a Python string literal.
fn escape_python_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_run_script_single_input() {
        let mut inputs = HashMap::new();
        inputs.insert("main".to_string(), PathBuf::from("/in/main.parquet"));
        let params = serde_json::json!({});

        let script = generate_run_script("clean", &inputs, &params).unwrap();
        assert!(script.contains("import transform"));
        assert!(script.contains("inputs[\"main\"] = pl.read_parquet(\"/in/main.parquet\")"));
        assert!(script.contains("result = transform.clean(inputs, params)"));
        assert!(script.contains("result.write_parquet(\"/out/output.parquet\")"));
    }

    #[test]
    fn test_generate_run_script_multi_input_sorted() {
        let mut inputs = HashMap::new();
        inputs.insert("right".to_string(), PathBuf::from("/in/right.parquet"));
        inputs.insert("left".to_string(), PathBuf::from("/in/left.parquet"));
        let params = serde_json::json!({"threshold": 0.5});

        let script = generate_run_script("merge", &inputs, &params).unwrap();
        // Left should appear before right (sorted)
        let left_idx = script.find("inputs[\"left\"]").unwrap();
        let right_idx = script.find("inputs[\"right\"]").unwrap();
        assert!(left_idx < right_idx);
        assert!(script.contains("threshold"));
    }

    #[test]
    fn test_generate_run_script_params_escaping() {
        let mut inputs = HashMap::new();
        inputs.insert("main".to_string(), PathBuf::from("/in/main.parquet"));
        let params = serde_json::json!({"key": "value with \"quotes\""});

        let script = generate_run_script("process", &inputs, &params).unwrap();
        // The JSON string should be embedded with escaped quotes
        assert!(script.contains("quotes"));
        // Should be parseable by json.loads in the generated script
        assert!(script.contains("json.loads("));
    }

    #[test]
    fn test_escape_python_string() {
        assert_eq!(escape_python_string("hello"), "hello");
        assert_eq!(escape_python_string("say \"hi\""), "say \\\"hi\\\"");
        assert_eq!(escape_python_string("line1\nline2"), "line1\\nline2");
        assert_eq!(escape_python_string("path\\to\\file"), "path\\\\to\\\\file");
    }
}
