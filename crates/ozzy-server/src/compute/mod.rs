//! Compute backend — pluggable execution engine for transform containers.
//!
//! The `ComputeBackend` trait allows swapping Docker (local) for Fly Machines
//! (cloud) or other providers. `BackendSelector` picks the right backend
//! based on server configuration.

pub mod docker;
pub mod orchestrator;
pub mod types;

pub use types::{ComputeBackend, ComputeRequest, ComputeResult, InputSpec};

/// Backend selector: wraps the active compute backend.
///
/// Constructed from `ComputeConfig` — returns `None` when compute is disabled.
/// Stored in `AppState` as `Option<BackendSelector>`.
#[derive(Clone)]
pub enum BackendSelector {
    Docker(docker::DockerBackend),
}

impl BackendSelector {
    /// Create a backend from server configuration.
    /// Returns `None` when compute is disabled.
    pub fn from_config(config: &crate::config::ComputeConfig) -> Option<Self> {
        if !config.enabled {
            return None;
        }
        Some(Self::Docker(docker::DockerBackend::new(
            config.tmpdir.clone(),
        )))
    }

    /// Execute a transform via the active backend.
    pub async fn run(&self, request: &ComputeRequest) -> anyhow::Result<ComputeResult> {
        match self {
            Self::Docker(backend) => {
                <docker::DockerBackend as ComputeBackend>::run(backend, request).await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert!(BackendSelector::from_config(&config).is_none());
    }

    #[test]
    fn test_backend_selector_enabled_docker() {
        let config = ComputeConfig {
            enabled: true,
            docker_runtime: Some("runsc".to_string()),
            memory_limit: "4g".to_string(),
            cpu_limit: "2".to_string(),
            timeout_secs: 600,
            tmpdir: "/opt/ozzy/tmp".to_string(),
            tmpfs_size: "1g".to_string(),
        };
        let backend = BackendSelector::from_config(&config);
        assert!(backend.is_some());
        match backend.unwrap() {
            BackendSelector::Docker(docker) => {
                assert_eq!(docker.tmpdir, "/opt/ozzy/tmp");
            }
        }
    }
}
