//! Fly Machines compute backend — runs transforms on Fly.io Firecracker VMs.
//!
//! The backend only handles machine lifecycle: create, wait for stop, collect
//! exit code + logs, destroy. All I/O (inputs, output, source, secrets) is
//! encoded in env_vars by the orchestrator using presigned URLs.

use std::time::Instant;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::types::{ComputeBackend, ComputeRequest, ComputeResult};
use crate::config::FlyConfig;

/// Fly Machines compute backend.
#[derive(Clone)]
pub struct FlyBackend {
    config: FlyConfig,
    http: reqwest::Client,
}

impl std::fmt::Debug for FlyBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlyBackend")
            .field("config", &self.config)
            .finish()
    }
}

impl FlyBackend {
    pub fn new(config: FlyConfig) -> Self {
        // No client-level timeout — each request sets its own .timeout() to avoid
        // a global 600s cap shadowing longer compute timeouts.
        let http = reqwest::Client::builder()
            .build()
            .expect("Failed to create HTTP client");
        Self { config, http }
    }
}

impl ComputeBackend for FlyBackend {
    fn run<'a>(
        &'a self,
        request: &'a ComputeRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ComputeResult>> + Send + 'a>>
    {
        Box::pin(self.run_inner(request))
    }
}

impl FlyBackend {
    async fn run_inner(&self, request: &ComputeRequest) -> Result<ComputeResult> {
        let start = Instant::now();
        let job_uuid = uuid::Uuid::new_v4();
        let machine_name = format!("ozzy-job-{}", job_uuid);

        // Build machine env — all env vars come from orchestrator
        let env: std::collections::HashMap<String, String> = request.env_vars.clone();

        let machine_config = CreateMachineRequest {
            name: machine_name.clone(),
            region: self.config.region.clone(),
            config: MachineConfig {
                image: request.image.clone(),
                auto_destroy: false,
                env,
                guest: GuestConfig {
                    cpu_kind: self.config.cpu_kind.clone(),
                    cpus: self.config.cpus,
                    memory_mb: self.config.memory_mb,
                },
                restart: RestartConfig {
                    policy: "no".into(),
                },
                // Decode init.sh from env and execute it
                init: InitConfig {
                    cmd: vec![
                        "/bin/sh".into(),
                        "-c".into(),
                        "echo \"$OZZY_INIT_SCRIPT_B64\" | base64 -d > /tmp/init.sh && /bin/sh /tmp/init.sh".into(),
                    ],
                },
            },
        };

        // Create machine
        let create_url = format!(
            "{}/v1/apps/{}/machines",
            self.config.api_url, self.config.app_name
        );

        let create_resp = match self
            .http
            .post(&create_url)
            .bearer_auth(&self.config.api_token)
            .json(&machine_config)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
        {
            Ok(resp) => resp,
            Err(e) => {
                tracing::error!(
                    url = %create_url,
                    is_connect = e.is_connect(),
                    is_timeout = e.is_timeout(),
                    "Fly Machine creation request failed: {e:#}"
                );
                return Err(e).context("Failed to create Fly Machine");
            }
        };

        if !create_resp.status().is_success() {
            let status = create_resp.status();
            let body = create_resp
                .text()
                .await
                .unwrap_or_else(|_| "no body".into());
            anyhow::bail!("Fly Machine creation failed ({}): {}", status, body);
        }

        let machine: CreateMachineResponse = create_resp
            .json()
            .await
            .context("Failed to parse Fly Machine creation response")?;
        let machine_id = machine.id;
        let instance_id = machine.instance_id;

        tracing::info!(
            "Fly Machine created: {} (id: {}, instance: {}, region: {})",
            machine_name,
            machine_id,
            instance_id,
            machine.region.as_deref().unwrap_or("unknown"),
        );

        // Wait for machine to stop
        let wait_result = self
            .wait_for_machine(&machine_id, &instance_id, request.timeout_secs)
            .await;

        // Get machine state for exit code
        let (exit_code, logs) = match &wait_result {
            Ok(()) => {
                let state = self.get_machine_state(&machine_id).await;
                match state {
                    Ok(ms) => {
                        let code = ms
                            .exit_code
                            .or_else(|| extract_exit_code_from_events(&ms.events))
                            .unwrap_or(-1);
                        let events_log = ms
                            .events
                            .iter()
                            .map(|e| {
                                format!(
                                    "[{}] {} {}",
                                    e.timestamp.as_deref().unwrap_or("?"),
                                    e.event_type.as_deref().unwrap_or("?"),
                                    e.message.as_deref().unwrap_or("")
                                )
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        (code, events_log)
                    }
                    Err(e) => {
                        tracing::warn!("Failed to get machine state for {}: {}", machine_id, e);
                        (-1, String::new())
                    }
                }
            }
            Err(e) => {
                tracing::error!("Fly Machine {} wait failed: {}", machine_id, e);
                let _ = self.destroy_machine(&machine_id).await;
                return Err(anyhow::anyhow!("Fly Machine execution failed: {}", e));
            }
        };

        // Destroy machine (best-effort)
        if let Err(e) = self.destroy_machine(&machine_id).await {
            tracing::warn!(
                "Failed to destroy Fly Machine {} (auto_destroy will clean up): {}",
                machine_id,
                e
            );
        }

        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(ComputeResult {
            exit_code,
            logs,
            duration_ms,
        })
    }
}

impl FlyBackend {
    /// Wait for a machine to reach "stopped" state.
    ///
    /// The Fly wait endpoint caps `timeout` at 60s, so we poll in a loop
    /// with 30s intervals until the overall deadline is reached.
    async fn wait_for_machine(
        &self,
        machine_id: &str,
        instance_id: &str,
        timeout_secs: u64,
    ) -> Result<()> {
        let deadline = Instant::now() + std::time::Duration::from_secs(timeout_secs);
        let poll_secs: u64 = 30;

        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                anyhow::bail!(
                    "Fly Machine {} timed out after {}s",
                    machine_id,
                    timeout_secs
                );
            }
            let this_timeout = remaining.as_secs().min(poll_secs).max(1);

            let wait_url = format!(
                "{}/v1/apps/{}/machines/{}/wait?state=stopped&timeout={}&instance_id={}",
                self.config.api_url, self.config.app_name, machine_id, this_timeout, instance_id
            );

            let resp = self
                .http
                .get(&wait_url)
                .bearer_auth(&self.config.api_token)
                .timeout(std::time::Duration::from_secs(this_timeout + 10))
                .send()
                .await
                .context("Fly Machine wait request failed")?;

            if resp.status().is_success() {
                return Ok(());
            }

            // 408 = timeout (machine still running), retry
            if resp.status() == reqwest::StatusCode::REQUEST_TIMEOUT {
                continue;
            }

            let status = resp.status();
            let body = resp.text().await.unwrap_or_else(|_| "no body".into());
            anyhow::bail!("Fly Machine wait failed ({}): {}", status, body);
        }
    }

    /// Get machine state (for exit code after stopping).
    async fn get_machine_state(&self, machine_id: &str) -> Result<MachineState> {
        let url = format!(
            "{}/v1/apps/{}/machines/{}",
            self.config.api_url, self.config.app_name, machine_id
        );

        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.config.api_token)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .context("Failed to get Fly Machine state")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_else(|_| "no body".into());
            anyhow::bail!("Failed to get machine state ({}): {}", status, body);
        }

        // Log raw response for debugging exit_code location in Fly API
        let body = resp
            .text()
            .await
            .context("Failed to read machine state body")?;
        tracing::debug!("Fly Machine {} state response: {}", machine_id, body);
        serde_json::from_str(&body).context("Failed to parse machine state response")
    }

    /// Destroy a machine (force=true to skip grace period).
    pub async fn destroy_machine(&self, machine_id: &str) -> Result<()> {
        let url = format!(
            "{}/v1/apps/{}/machines/{}?force=true",
            self.config.api_url, self.config.app_name, machine_id
        );

        let resp = self
            .http
            .delete(&url)
            .bearer_auth(&self.config.api_token)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .context("Failed to destroy Fly Machine")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_else(|_| "no body".into());
            anyhow::bail!("Failed to destroy machine ({}): {}", status, body);
        }

        Ok(())
    }

    /// List all machines in the app.
    pub async fn list_machines(&self) -> Result<Vec<MachineListEntry>> {
        let url = format!(
            "{}/v1/apps/{}/machines",
            self.config.api_url, self.config.app_name
        );

        let resp = match self
            .http
            .get(&url)
            .bearer_auth(&self.config.api_token)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("Fly list machines request failed: {e:#}");
                return Err(e).context("Failed to list Fly Machines");
            }
        };

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_else(|_| "no body".into());
            anyhow::bail!("Failed to list machines ({}): {}", status, body);
        }

        resp.json()
            .await
            .context("Failed to parse machine list response")
    }

    /// Find and destroy orphaned Fly machines.
    ///
    /// An orphan is any `ozzy-job-*` machine older than `max_age`. These are
    /// machines whose orchestrator crashed, timed out, or failed to clean up.
    /// Called periodically from a background task.
    pub async fn cleanup_orphans(&self, max_age: std::time::Duration) -> Result<u32> {
        let machines = self.list_machines().await?;
        let cutoff = chrono::Utc::now() - chrono::Duration::from_std(max_age)?;
        let mut destroyed = 0u32;

        for machine in &machines {
            // Only touch our machines
            let name = machine.name.as_deref().unwrap_or("");
            if !name.starts_with("ozzy-job-") {
                continue;
            }

            // Parse created_at timestamp
            let created_at = match &machine.created_at {
                Some(ts) => match ts.parse::<chrono::DateTime<chrono::Utc>>() {
                    Ok(dt) => dt,
                    Err(_) => continue, // Can't determine age, skip
                },
                None => continue,
            };

            if created_at < cutoff {
                tracing::warn!(
                    "Destroying orphaned Fly Machine: {} (id: {}, state: {:?}, created: {})",
                    name,
                    machine.id,
                    machine.state,
                    created_at,
                );
                if let Err(e) = self.destroy_machine(&machine.id).await {
                    tracing::error!("Failed to destroy orphan {}: {}", machine.id, e);
                } else {
                    destroyed += 1;
                }
            }
        }

        if destroyed > 0 {
            tracing::info!("Orphan cleanup: destroyed {} machines", destroyed);
        }

        Ok(destroyed)
    }
}

// ── Fly Machines API types ──────────────────────────────────────

#[derive(Debug, Serialize)]
struct CreateMachineRequest {
    name: String,
    region: String,
    config: MachineConfig,
}

#[derive(Debug, Serialize)]
struct MachineConfig {
    image: String,
    auto_destroy: bool,
    env: std::collections::HashMap<String, String>,
    guest: GuestConfig,
    restart: RestartConfig,
    init: InitConfig,
}

#[derive(Debug, Serialize)]
struct GuestConfig {
    cpu_kind: String,
    cpus: u32,
    memory_mb: u32,
}

#[derive(Debug, Serialize)]
struct RestartConfig {
    policy: String,
}

#[derive(Debug, Serialize)]
struct InitConfig {
    cmd: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CreateMachineResponse {
    id: String,
    instance_id: String,
    #[allow(dead_code)]
    name: Option<String>,
    region: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MachineState {
    #[serde(default)]
    events: Vec<MachineEvent>,
    #[serde(default, deserialize_with = "deserialize_exit_code")]
    exit_code: Option<i32>,
}

fn deserialize_exit_code<'de, D>(deserializer: D) -> std::result::Result<Option<i32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<i32>::deserialize(deserializer).or(Ok(None))
}

#[derive(Debug, Deserialize)]
struct MachineEvent {
    #[serde(rename = "type")]
    event_type: Option<String>,
    #[serde(default, deserialize_with = "deserialize_timestamp")]
    timestamp: Option<String>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    request: Option<EventRequest>,
}

/// Accept timestamp as either a string or an integer (millis since epoch).
fn deserialize_timestamp<'de, D>(deserializer: D) -> std::result::Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrNumber {
        Str(String),
        Num(i64),
    }
    match Option::<StringOrNumber>::deserialize(deserializer)? {
        Some(StringOrNumber::Str(s)) => Ok(Some(s)),
        Some(StringOrNumber::Num(n)) => Ok(Some(n.to_string())),
        None => Ok(None),
    }
}

#[derive(Debug, Deserialize)]
struct EventRequest {
    #[serde(default)]
    exit_event: Option<ExitEvent>,
}

#[derive(Debug, Deserialize)]
struct ExitEvent {
    exit_code: Option<i32>,
    guest_exit_code: Option<i32>,
}

/// Machine entry from list endpoint (minimal fields).
#[derive(Debug, Deserialize)]
pub struct MachineListEntry {
    pub id: String,
    pub name: Option<String>,
    pub state: Option<String>,
    pub created_at: Option<String>,
}

/// Extract exit code from machine events as a fallback.
///
/// Looks for the last event of type "exit" and returns its exit code.
/// Uses `exit_code` (the user command's actual exit code) as the primary source.
/// Falls back to `guest_exit_code` (Fly init system exit) if `exit_code` is missing.
fn extract_exit_code_from_events(events: &[MachineEvent]) -> Option<i32> {
    events
        .iter()
        .rev()
        .find(|e| e.event_type.as_deref() == Some("exit"))
        .and_then(|e| e.request.as_ref())
        .and_then(|r| r.exit_event.as_ref())
        .and_then(|ee| ee.exit_code.or(ee.guest_exit_code))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_machine_request_serialization() {
        let mut env = std::collections::HashMap::new();
        env.insert("PYTHONHASHSEED".into(), "0".into());
        env.insert("OZZY_PARAMS".into(), "{}".into());

        let req = CreateMachineRequest {
            name: "ozzy-job-test".into(),
            region: "fra".into(),
            config: MachineConfig {
                image: "registry.fly.io/ozzydb-compute:abc123".into(),
                auto_destroy: true,
                env,
                guest: GuestConfig {
                    cpu_kind: "shared".into(),
                    cpus: 1,
                    memory_mb: 512,
                },
                restart: RestartConfig {
                    policy: "no".into(),
                },
                init: InitConfig {
                    cmd: vec!["/bin/sh".into(), "-c".into(), "echo test".into()],
                },
            },
        };

        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["name"], "ozzy-job-test");
        assert_eq!(json["region"], "fra");
        assert_eq!(
            json["config"]["image"],
            "registry.fly.io/ozzydb-compute:abc123"
        );
        assert_eq!(json["config"]["auto_destroy"], true);
        assert_eq!(json["config"]["guest"]["cpu_kind"], "shared");
        assert_eq!(json["config"]["guest"]["cpus"], 1);
        assert_eq!(json["config"]["guest"]["memory_mb"], 512);
        assert_eq!(json["config"]["restart"]["policy"], "no");
        assert_eq!(json["config"]["init"]["cmd"][0], "/bin/sh");
    }

    #[test]
    fn test_machine_state_deserialization() {
        let json = r#"{
            "id": "abc123",
            "state": "stopped",
            "events": [
                {"type": "start", "timestamp": "2026-01-01T00:00:00Z"},
                {"type": "exit", "timestamp": "2026-01-01T00:01:00Z", "message": "exit code: 0"}
            ],
            "exit_code": 0
        }"#;

        let state: MachineState = serde_json::from_str(json).unwrap();
        assert_eq!(state.exit_code, Some(0));
        assert_eq!(state.events.len(), 2);
        assert_eq!(state.events[0].event_type.as_deref(), Some("start"));
    }

    #[test]
    fn test_machine_state_exit_code_from_events() {
        // When exit_code is not top-level, extract from events
        let json = r#"{
            "id": "abc123",
            "state": "stopped",
            "events": [
                {"type": "start", "timestamp": "2026-01-01T00:00:00Z"},
                {"type": "exit", "timestamp": "2026-01-01T00:01:00Z", "request": {"exit_event": {"exit_code": 0}}}
            ]
        }"#;

        let state: MachineState = serde_json::from_str(json).unwrap();
        assert_eq!(state.exit_code, None);
        assert_eq!(extract_exit_code_from_events(&state.events), Some(0));
    }

    #[test]
    fn test_machine_state_exit_code_from_events_nonzero() {
        let json = r#"{
            "events": [
                {"type": "start", "timestamp": "2026-01-01T00:00:00Z"},
                {"type": "exit", "timestamp": "2026-01-01T00:01:00Z", "request": {"exit_event": {"exit_code": 137}}}
            ]
        }"#;

        let state: MachineState = serde_json::from_str(json).unwrap();
        assert_eq!(extract_exit_code_from_events(&state.events), Some(137));
    }

    #[test]
    fn test_exit_code_preferred_over_guest_exit_code() {
        // exit_code is the user command's actual exit code;
        // guest_exit_code is the Fly init system's exit code.
        // We prefer exit_code as it reflects the actual command result.
        let json = r#"{
            "events": [
                {"type": "start", "timestamp": 1771113249144},
                {"type": "exit", "timestamp": 1771113250634, "request": {"exit_event": {"exit_code": 2, "guest_exit_code": 0}}}
            ]
        }"#;

        let state: MachineState = serde_json::from_str(json).unwrap();
        assert_eq!(extract_exit_code_from_events(&state.events), Some(2));
    }

    #[test]
    fn test_guest_exit_code_fallback() {
        // When exit_code is missing, fall back to guest_exit_code
        let json = r#"{
            "events": [
                {"type": "exit", "timestamp": 1771113250634, "request": {"exit_event": {"guest_exit_code": 1}}}
            ]
        }"#;

        let state: MachineState = serde_json::from_str(json).unwrap();
        assert_eq!(extract_exit_code_from_events(&state.events), Some(1));
    }

    #[test]
    fn test_machine_state_no_exit_code() {
        let json = r#"{
            "id": "abc123",
            "state": "running",
            "events": []
        }"#;

        let state: MachineState = serde_json::from_str(json).unwrap();
        assert_eq!(state.exit_code, None);
    }

    #[test]
    fn test_orphan_detection_logic() {
        // Verify the naming convention and age check that cleanup_orphans uses
        let now = chrono::Utc::now();
        let old = now - chrono::Duration::minutes(45);
        let recent = now - chrono::Duration::minutes(5);
        let max_age = std::time::Duration::from_secs(30 * 60); // 30 min
        let cutoff = now - chrono::Duration::from_std(max_age).unwrap();

        // Old ozzy-job machine → orphan
        let m1 = MachineListEntry {
            id: "m1".into(),
            name: Some("ozzy-job-abc".into()),
            state: Some("stopped".into()),
            created_at: Some(old.to_rfc3339()),
        };
        assert!(m1.name.as_deref().unwrap().starts_with("ozzy-job-"));
        let ts1: chrono::DateTime<chrono::Utc> = m1.created_at.as_deref().unwrap().parse().unwrap();
        assert!(ts1 < cutoff, "Old machine should be before cutoff");

        // Recent ozzy-job machine → not an orphan
        let m2 = MachineListEntry {
            id: "m2".into(),
            name: Some("ozzy-job-def".into()),
            state: Some("running".into()),
            created_at: Some(recent.to_rfc3339()),
        };
        let ts2: chrono::DateTime<chrono::Utc> = m2.created_at.as_deref().unwrap().parse().unwrap();
        assert!(ts2 >= cutoff, "Recent machine should be after cutoff");

        // Non-ozzy machine → skipped
        let m3 = MachineListEntry {
            id: "m3".into(),
            name: Some("other-app".into()),
            state: Some("running".into()),
            created_at: Some(old.to_rfc3339()),
        };
        assert!(!m3.name.as_deref().unwrap().starts_with("ozzy-job-"));
    }

    #[test]
    fn test_machine_list_entry_deserialization() {
        let json = r#"[
            {"id": "m1", "name": "ozzy-job-abc", "state": "stopped", "created_at": "2026-01-01T00:00:00Z"},
            {"id": "m2", "name": "ozzy-job-def", "state": "running", "created_at": "2026-01-01T00:01:00Z"}
        ]"#;

        let machines: Vec<MachineListEntry> = serde_json::from_str(json).unwrap();
        assert_eq!(machines.len(), 2);
        assert_eq!(machines[0].id, "m1");
        assert_eq!(machines[0].state.as_deref(), Some("stopped"));
    }
}
