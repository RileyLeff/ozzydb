//! Docker compute backend — runs transforms in Docker containers.
//!
//! The backend only handles container lifecycle: create, run, wait, collect
//! exit code + logs. All I/O (inputs, output, source, secrets) is encoded
//! in env_vars by the orchestrator using presigned URLs.

use std::time::Instant;

use anyhow::{Context, Result};
use tokio::process::Command;

use super::types::{ComputeBackend, ComputeRequest, ComputeResult};

/// Docker compute backend: runs transforms in local Docker containers.
#[derive(Clone)]
pub struct DockerBackend {
    pub config: crate::config::DockerProviderConfig,
}

impl std::fmt::Debug for DockerBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DockerBackend")
            .field("runtime", &self.config.runtime)
            .finish()
    }
}

impl DockerBackend {
    pub fn new(config: crate::config::DockerProviderConfig) -> Self {
        Self { config }
    }
}

impl ComputeBackend for DockerBackend {
    fn run<'a>(
        &'a self,
        request: &'a ComputeRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ComputeResult>> + Send + 'a>>
    {
        Box::pin(self.run_inner(request))
    }
}

impl DockerBackend {
    async fn run_inner(&self, request: &ComputeRequest) -> Result<ComputeResult> {
        let start = Instant::now();

        let container_id = uuid::Uuid::new_v4().to_string();
        let short_id = container_id.get(..8).unwrap_or(&container_id);
        let container_name = format!("ozzydb-{}", short_id);

        let mut cmd = Command::new("docker");
        cmd.arg("run").arg("--rm").args(["--name", &container_name]);

        // Resource limits from provider config
        cmd.args(["--memory", &self.config.memory_limit]);
        cmd.args(["--cpus", &self.config.cpu_limit]);

        // Runtime (e.g., gVisor) — from backend config
        if let Some(ref runtime) = self.config.runtime {
            cmd.args(["--runtime", runtime]);
        }

        if !request.network_enabled {
            cmd.args(["--network", "none"]);
        }

        // All env vars from orchestrator (includes OZZY_*, determinism vars, etc.)
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
            Ok(result) => result.context("Failed to execute docker run")?,
            Err(_) => {
                // Timeout: kill container
                let _ = tokio::process::Command::new("docker")
                    .args(["kill", &container_name])
                    .output()
                    .await;
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

        Ok(ComputeResult {
            exit_code,
            logs,
            duration_ms,
        })
    }
}
