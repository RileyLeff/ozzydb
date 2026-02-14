//! Docker compute backend — runs transforms in Docker containers.
//!
//! Used both server-side (gVisor sandbox) and by `ozzy run` (local Docker).
//! Inputs are bind-mounted, the runner script is written to the workspace,
//! and output is collected from the workspace after execution.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use tokio::process::Command;

use super::types::{ComputeBackend, ComputeRequest, ComputeResult};

/// Docker compute backend: runs transforms in local Docker containers.
///
/// Holds the tmpdir path for creating workspaces. All other per-job config
/// (image, timeout, resource limits, runtime) comes from `ComputeRequest`.
#[derive(Debug, Clone)]
pub struct DockerBackend {
    /// Temporary directory for compute workspaces.
    pub tmpdir: String,
}

impl DockerBackend {
    pub fn new(tmpdir: String) -> Self {
        Self { tmpdir }
    }
}

impl ComputeBackend for DockerBackend {
    async fn run(&self, request: &ComputeRequest) -> anyhow::Result<ComputeResult> {
        run(request, &self.tmpdir).await
    }
}

/// Execute a transform in a Docker container.
///
/// 1. Creates a workspace directory in tmpdir
/// 2. Writes runner + init scripts to the workspace
/// 3. Copies/symlinks inputs to the workspace
/// 4. Runs `docker run` with bind mounts, env vars, and resource limits
/// 5. Collects output from the workspace
/// 6. Returns the result
pub async fn run(request: &ComputeRequest, tmpdir: &str) -> Result<ComputeResult> {
    let start = Instant::now();

    // Create unique workspace directory
    let workspace_id = uuid::Uuid::new_v4().to_string();
    let workspace = PathBuf::from(tmpdir).join(&workspace_id);
    tokio::fs::create_dir_all(&workspace)
        .await
        .context("Failed to create workspace directory")?;

    // Workspace cleanup is deferred until after the caller reads output.
    // See cleanup_workspace() — called via ComputeResult::cleanup().

    // Create workspace subdirectories
    let inputs_dir = workspace.join("inputs");
    let output_dir = workspace.join("output");
    let source_dir = workspace.join("source");

    // Set up workspace — clean up on any setup failure
    if let Err(e) =
        setup_workspace(request, &workspace, &inputs_dir, &output_dir, &source_dir).await
    {
        cleanup_workspace(workspace).await;
        return Err(e);
    }

    // Build docker run command
    let short_id = workspace_id.get(..8).unwrap_or(&workspace_id);
    let container_name = format!("ozzydb-{}", short_id);
    let mut cmd = Command::new("docker");
    cmd.arg("run").arg("--rm").args(["--name", &container_name]);

    // Resource limits
    if let Some(ref mem) = request.memory_limit {
        cmd.args(["--memory", mem]);
    }
    if let Some(ref cpu) = request.cpu_limit {
        cmd.args(["--cpus", cpu]);
    }

    // Runtime (e.g., gVisor)
    if let Some(ref runtime) = request.runtime {
        cmd.args(["--runtime", runtime]);
    }

    // Network isolation
    if !request.network {
        cmd.args(["--network", "none"]);
    }

    // Bind mounts
    cmd.args(["-v", &format!("{}:/workspace:rw", workspace.display())]);

    // Determinism env vars (always set)
    cmd.args(["-e", "PYTHONHASHSEED=0"]);
    cmd.args(["-e", "OMP_NUM_THREADS=1"]);
    cmd.args(["-e", "MKL_NUM_THREADS=1"]);
    cmd.args(["-e", "OPENBLAS_NUM_THREADS=1"]);
    cmd.args(["-e", "NUMEXPR_NUM_THREADS=1"]);
    cmd.args(["-e", "VECLIB_MAXIMUM_THREADS=1"]);

    // User env vars (OZZY_PARAMS, OZZY_INPUT_MANIFEST, OZZY_PARAM_*, secrets)
    for (key, value) in &request.env_vars {
        cmd.args(["-e", &format!("{}={}", key, value)]);
    }

    // Image and entrypoint
    cmd.arg(&request.image);
    cmd.args(["/bin/sh", "/workspace/init.sh"]);

    // Execute with timeout — kill child and clean up workspace on failure.
    // We use spawn() + wait_with_output() so we can kill the child on timeout.
    let child = cmd
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("Failed to spawn docker run")?;

    let output = match tokio::time::timeout(
        std::time::Duration::from_secs(request.timeout_secs),
        child.wait_with_output(),
    )
    .await
    {
        Ok(result) => match result {
            Ok(output) => output,
            Err(e) => {
                cleanup_workspace(workspace).await;
                return Err(e).context("Failed to execute docker run");
            }
        },
        Err(_) => {
            // Timeout: kill the child process. wait_with_output() was cancelled
            // by the timeout, but it takes ownership of child. We need to
            // forcefully stop the container instead.
            // Reuse container_name from line 51 (same short_id prefix).
            let _ = tokio::process::Command::new("docker")
                .args(["kill", &container_name])
                .output()
                .await;
            cleanup_workspace(workspace).await;
            anyhow::bail!(
                "Transform execution timed out after {}s",
                request.timeout_secs
            );
        }
    };

    let duration_ms = start.elapsed().as_millis() as u64;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let logs = format!("{}{}", stdout, stderr);

    Ok(ComputeResult {
        output_dir,
        workspace_dir: workspace,
        exit_code: output.status.code().unwrap_or(-1),
        logs,
        duration_ms,
    })
}

/// Set up the workspace: create dirs, write scripts, copy inputs/source.
async fn setup_workspace(
    request: &ComputeRequest,
    workspace: &Path,
    inputs_dir: &Path,
    output_dir: &Path,
    source_dir: &Path,
) -> Result<()> {
    tokio::fs::create_dir_all(inputs_dir).await?;
    tokio::fs::create_dir_all(output_dir).await?;
    tokio::fs::create_dir_all(source_dir).await?;

    // Write runner script
    let runner_path = workspace.join(format!("runner.{}", request.runner_ext));
    tokio::fs::write(&runner_path, &request.runner_script).await?;

    // Write init script
    let init_path = workspace.join("init.sh");
    tokio::fs::write(&init_path, &request.init_script).await?;

    // Link/copy inputs to workspace
    for input in &request.inputs {
        let dest = inputs_dir.join(&input.name);
        if input.is_collection {
            copy_dir(&input.local_path, &dest).await?;
        } else {
            if tokio::fs::hard_link(&input.local_path, &dest)
                .await
                .is_err()
            {
                tokio::fs::copy(&input.local_path, &dest).await?;
            }
        }
    }

    // If source directory is provided, copy it
    if let Some(src) = &request.source_dir {
        copy_dir(src, source_dir).await?;
    }

    Ok(())
}

/// Recursively copy a directory, skipping symlinks.
async fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    tokio::fs::create_dir_all(dst).await?;
    let mut entries = tokio::fs::read_dir(src).await?;
    while let Some(entry) = entries.next_entry().await? {
        let ft = entry.file_type().await?;
        // Skip symlinks to prevent sandbox data exfiltration
        if ft.is_symlink() {
            tracing::warn!("Skipping symlink in copy_dir: {}", entry.path().display());
            continue;
        }
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if ft.is_dir() {
            Box::pin(copy_dir(&src_path, &dst_path)).await?;
        } else if ft.is_file() {
            tokio::fs::copy(&src_path, &dst_path).await?;
        }
    }
    Ok(())
}

/// Best-effort async cleanup for workspace directories.
///
/// Called explicitly after the caller has finished reading from the workspace.
/// Using tokio::spawn instead of Drop to avoid racing with output consumption.
pub async fn cleanup_workspace(path: PathBuf) {
    let _ = tokio::fs::remove_dir_all(&path).await;
}

/// Build the input manifest JSON for a set of inputs.
///
/// This produces the JSON blob that the runner reads from `OZZY_INPUT_MANIFEST`.
pub fn build_input_manifest(inputs: &[super::types::InputSpec]) -> serde_json::Value {
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
pub fn build_param_env_vars(params: &serde_json::Value) -> Vec<(String, String)> {
    let mut vars = Vec::new();
    if let Some(obj) = params.as_object() {
        for (key, value) in obj {
            let str_value = match value {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            vars.push((format!("OZZY_PARAM_{}", key), str_value));
        }
    }
    vars
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compute::types::InputSpec;
    use std::path::PathBuf;

    #[test]
    fn test_build_input_manifest_single() {
        let inputs = vec![InputSpec {
            name: "readings".to_string(),
            local_path: PathBuf::from("/data/readings.parquet"),
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
            local_path: PathBuf::from("/data/all_readings/"),
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

        // Check that all params are present (order may vary)
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
        // String values should not be JSON-quoted
        assert_eq!(vars[0].1, "alice");
    }
}
