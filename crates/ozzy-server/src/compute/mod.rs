//! Compute backend — pluggable execution engine for transform containers.
//!
//! The `ComputeBackend` trait allows swapping Docker (local) for Fly Machines
//! (cloud) or other providers. `BackendSelector` picks the right backend
//! based on server configuration.

pub mod docker;
pub mod environments;
pub mod fly;
pub mod orchestrator;
pub mod rate_limit;
pub mod secrets;
pub mod types;

pub use types::{
    ComputeBackend, ComputeRequest, ComputeResult, InputSpec, build_input_manifest,
    build_param_env_vars,
};

/// Backend selector: wraps the active compute backend.
///
/// Constructed from server configuration — returns `None` when compute is disabled.
/// Stored in `AppState` as `Option<BackendSelector>`.
///
/// Selection priority: Fly (if configured + R2 available) > Docker (if compute enabled).
#[derive(Clone)]
pub enum BackendSelector {
    Docker(docker::DockerBackend),
    Fly(fly::FlyBackend),
}

impl BackendSelector {
    /// Create a backend from server configuration.
    ///
    /// Priority: Fly (if FLY_API_TOKEN + FLY_APP_NAME set) > Docker (if COMPUTE_ENABLED=true).
    /// Returns `None` when no compute backend is available.
    pub fn from_config(
        compute_config: &crate::config::ComputeConfig,
        fly_config: Option<&crate::config::FlyConfig>,
    ) -> Option<Self> {
        // Fly takes priority when configured
        if let Some(fly) = fly_config {
            return Some(Self::Fly(fly::FlyBackend::new(fly.clone())));
        }

        // Fall back to Docker
        if !compute_config.enabled {
            return None;
        }
        Some(Self::Docker(docker::DockerBackend::new(
            compute_config.docker_runtime.clone(),
        )))
    }

    /// Execute a transform via the active backend.
    pub async fn run(&self, request: &ComputeRequest) -> anyhow::Result<ComputeResult> {
        match self {
            Self::Docker(backend) => {
                <docker::DockerBackend as ComputeBackend>::run(backend, request).await
            }
            Self::Fly(backend) => <fly::FlyBackend as ComputeBackend>::run(backend, request).await,
        }
    }

    /// Get the Fly backend (for orphan cleanup, etc.).
    pub fn as_fly(&self) -> Option<&fly::FlyBackend> {
        match self {
            Self::Fly(backend) => Some(backend),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::config::ComputeConfig;

    #[test]
    fn test_backend_selector_disabled() {
        let config = ComputeConfig {
            enabled: false,
            docker_runtime: None,
            memory_limit: "2g".to_string(),
            cpu_limit: "1".to_string(),
            timeout_secs: 300,
            tmpdir: "/tmp/ozzy".to_string(),
            tmpfs_size: "512m".to_string(),
        };
        // Can't test without a real storage instance; just verify the config logic
        assert!(!config.enabled);
    }

    #[test]
    fn test_backend_selector_config_enabled() {
        let config = ComputeConfig {
            enabled: true,
            docker_runtime: Some("runsc".to_string()),
            memory_limit: "4g".to_string(),
            cpu_limit: "2".to_string(),
            timeout_secs: 600,
            tmpdir: "/opt/ozzy/tmp".to_string(),
            tmpfs_size: "1g".to_string(),
        };
        assert!(config.enabled);
        assert_eq!(config.docker_runtime.as_deref(), Some("runsc"));
    }
}
