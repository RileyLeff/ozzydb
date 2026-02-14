//! Docker compute backend — runs transforms in Docker containers.
//!
//! All I/O happens via presigned URLs, just like Fly Machines. The container
//! downloads inputs, runs the transform, and uploads output. No bind mounts.

use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result};
use tokio::process::Command;

use super::types::{ComputeBackend, ComputeRequest, ComputeResult};
use crate::storage::ContentStorage;

/// Docker compute backend: runs transforms in local Docker containers.
#[derive(Clone)]
pub struct DockerBackend {
    /// Temporary directory for extracting output after download.
    pub tmpdir: String,
    /// Storage backend for output download (presigned URLs and retrieval).
    pub storage: ContentStorage,
}

impl std::fmt::Debug for DockerBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DockerBackend")
            .field("tmpdir", &self.tmpdir)
            .finish()
    }
}

impl DockerBackend {
    pub fn new(tmpdir: String, storage: ContentStorage) -> Self {
        Self { tmpdir, storage }
    }
}

impl ComputeBackend for DockerBackend {
    async fn run(&self, request: &ComputeRequest) -> anyhow::Result<ComputeResult> {
        run(request, &self.tmpdir, &self.storage).await
    }
}

/// Execute a transform in a Docker container using presigned URL I/O.
///
/// 1. Generates a presigned PUT URL for the output tarball
/// 2. Runs `docker run` with env vars (no bind mounts)
/// 3. Container downloads inputs, runs transform, uploads output via init script
/// 4. After exit, downloads output from R2 and extracts to local temp dir
/// 5. Returns the result with output_dir pointing to extracted files
pub async fn run(
    request: &ComputeRequest,
    tmpdir: &str,
    storage: &ContentStorage,
) -> Result<ComputeResult> {
    let start = Instant::now();

    // Create workspace for extracting output after download
    let workspace_id = uuid::Uuid::new_v4().to_string();
    let workspace = PathBuf::from(tmpdir).join(&workspace_id);
    tokio::fs::create_dir_all(&workspace)
        .await
        .context("Failed to create workspace directory")?;
    let output_dir = workspace.join("output");
    tokio::fs::create_dir_all(&output_dir).await?;

    // Generate a temp R2 key for the output tarball
    let output_temp_key = format!("docker-output/{}.tar.gz", workspace_id);
    let put_ttl = std::time::Duration::from_secs(request.timeout_secs + 300);
    let output_upload_url = storage
        .presigned_put_url_for_compute(&output_temp_key, put_ttl)
        .await
        .context("Failed to generate presigned PUT URL for output")?;

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

    // Network access is always needed for presigned URL I/O.
    // (--network none is NOT used; sandboxing is handled by gVisor if configured.)

    // Determinism env vars (always set)
    cmd.args(["-e", "PYTHONHASHSEED=0"]);
    cmd.args(["-e", "OMP_NUM_THREADS=1"]);
    cmd.args(["-e", "MKL_NUM_THREADS=1"]);
    cmd.args(["-e", "OPENBLAS_NUM_THREADS=1"]);
    cmd.args(["-e", "NUMEXPR_NUM_THREADS=1"]);
    cmd.args(["-e", "VECLIB_MAXIMUM_THREADS=1"]);

    // Base64-encode init script and runner script into env vars
    let init_b64 = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        &request.init_script,
    );
    cmd.args(["-e", &format!("OZZY_INIT_SCRIPT_B64={}", init_b64)]);

    let runner_b64 = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        &request.runner_script,
    );
    cmd.args(["-e", &format!("OZZY_RUNNER_SCRIPT_B64={}", runner_b64)]);

    // Output upload URL
    cmd.args([
        "-e",
        &format!("OZZY_OUTPUT_UPLOAD_URL={}", output_upload_url),
    ]);

    // User env vars (OZZY_PARAMS, OZZY_INPUT_MANIFEST, OZZY_PARAM_*,
    // OZZY_INPUT_DOWNLOADS, OZZY_SOURCE_DOWNLOAD, OZZY_SECRETS_URL, etc.)
    for (key, value) in &request.env_vars {
        cmd.args(["-e", &format!("{}={}", key, value)]);
    }

    // Image and entrypoint: decode init script from env var, run it
    cmd.arg(&request.image);
    cmd.args([
        "/bin/sh",
        "-c",
        "echo \"$OZZY_INIT_SCRIPT_B64\" | base64 -d > /tmp/init.sh && /bin/sh /tmp/init.sh",
    ]);

    // Execute with timeout
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
                let _ = storage.delete_by_key(&output_temp_key).await;
                return Err(e).context("Failed to execute docker run");
            }
        },
        Err(_) => {
            // Timeout: kill container
            let _ = tokio::process::Command::new("docker")
                .args(["kill", &container_name])
                .output()
                .await;
            cleanup_workspace(workspace).await;
            let _ = storage.delete_by_key(&output_temp_key).await;
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
    let exit_code = output.status.code().unwrap_or(-1);

    // If transform failed, don't try to download output
    if exit_code != 0 {
        let _ = storage.delete_by_key(&output_temp_key).await;
        return Ok(ComputeResult {
            output_dir,
            workspace_dir: workspace,
            exit_code,
            logs,
            duration_ms,
        });
    }

    // Download output tarball from R2 and extract
    if let Err(e) = download_and_extract_output(storage, &output_temp_key, &output_dir).await {
        tracing::error!("Failed to download/extract Docker output: {}", e);
        let _ = storage.delete_by_key(&output_temp_key).await;
        cleanup_workspace(workspace).await;
        return Err(e.context("Failed to download/extract Docker container output"));
    }

    // Clean up temp tarball from R2
    let _ = storage.delete_by_key(&output_temp_key).await;

    Ok(ComputeResult {
        output_dir,
        workspace_dir: workspace,
        exit_code,
        logs,
        duration_ms,
    })
}

/// Download output tarball from R2 and extract to output_dir.
async fn download_and_extract_output(
    storage: &ContentStorage,
    temp_key: &str,
    output_dir: &std::path::Path,
) -> Result<()> {
    let tarball_bytes = storage.get_by_key(temp_key).await?;

    let cursor = std::io::Cursor::new(&tarball_bytes);
    let gz = flate2::read::GzDecoder::new(cursor);
    let mut archive = tar::Archive::new(gz);
    archive.set_preserve_permissions(false);
    archive.set_unpack_xattrs(false);
    archive.set_overwrite(false);

    archive
        .unpack(output_dir)
        .context("Failed to extract output tarball")?;

    tracing::debug!(
        "Extracted Docker output ({} bytes) to {}",
        tarball_bytes.len(),
        output_dir.display()
    );

    Ok(())
}

/// Best-effort async cleanup for workspace directories.
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
