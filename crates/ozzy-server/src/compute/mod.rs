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

pub use types::{ComputeBackend, ComputeRequest, ComputeResult, InputSpec};

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
    /// Priority: Fly (if FLY_API_TOKEN + FLY_APP_NAME set + R2 configured) > Docker (if COMPUTE_ENABLED=true).
    /// Returns `None` when no compute backend is available.
    pub fn from_config(
        compute_config: &crate::config::ComputeConfig,
        fly_config: Option<&crate::config::FlyConfig>,
        storage: Option<&crate::storage::ContentStorage>,
    ) -> Option<Self> {
        // Fly takes priority when configured (requires R2 storage for presigned URLs)
        if let Some(fly) = fly_config {
            if let Some(storage) = storage {
                if storage.has_remote() {
                    return Some(Self::Fly(fly::FlyBackend::new(
                        fly.clone(),
                        storage.clone(),
                        compute_config.tmpdir.clone(),
                    )));
                }
                tracing::warn!(
                    "Fly config present but R2 not configured — falling back to Docker"
                );
            } else {
                tracing::warn!(
                    "Fly config present but no storage available — falling back to Docker"
                );
            }
        }

        // Fall back to Docker
        if !compute_config.enabled {
            return None;
        }
        Some(Self::Docker(docker::DockerBackend::new(
            compute_config.tmpdir.clone(),
        )))
    }

    /// Execute a transform via the active backend.
    pub async fn run(&self, request: &ComputeRequest) -> anyhow::Result<ComputeResult> {
        match self {
            Self::Docker(backend) => {
                <docker::DockerBackend as ComputeBackend>::run(backend, request).await
            }
            Self::Fly(backend) => {
                <fly::FlyBackend as ComputeBackend>::run(backend, request).await
            }
        }
    }

    /// Whether this is a Fly backend (affects orchestrator behavior).
    pub fn is_fly(&self) -> bool {
        matches!(self, Self::Fly(_))
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
        assert!(BackendSelector::from_config(&config, None, None).is_none());
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
        let backend = BackendSelector::from_config(&config, None, None);
        assert!(backend.is_some());
        assert!(!backend.as_ref().unwrap().is_fly());
        match backend.unwrap() {
            BackendSelector::Docker(docker) => {
                assert_eq!(docker.tmpdir, "/opt/ozzy/tmp");
            }
            _ => panic!("Expected Docker backend"),
        }
    }

    #[test]
    fn test_backend_selector_fly_without_storage_falls_back_to_docker() {
        // Fly is configured but no R2 storage — should fall back to Docker
        let compute_config = ComputeConfig {
            enabled: true,
            docker_runtime: None,
            memory_limit: "2g".to_string(),
            cpu_limit: "1".to_string(),
            timeout_secs: 300,
            tmpdir: "/tmp/ozzy".to_string(),
            tmpfs_size: "512m".to_string(),
        };
        let fly_config = crate::config::FlyConfig {
            api_token: "fly_test".into(),
            app_name: "test-app".into(),
            api_url: "https://api.machines.dev".into(),
            region: "fra".into(),
            cpu_kind: "shared".into(),
            cpus: 1,
            memory_mb: 512,
        };
        let backend =
            BackendSelector::from_config(&compute_config, Some(&fly_config), None);
        // Falls back to Docker since no storage is available
        assert!(backend.is_some());
        assert!(!backend.unwrap().is_fly());
    }

    #[test]
    fn test_backend_selector_fly_without_docker_or_storage() {
        // Fly configured, Docker disabled, no storage — no backend available
        let compute_config = ComputeConfig {
            enabled: false,
            docker_runtime: None,
            memory_limit: "2g".to_string(),
            cpu_limit: "1".to_string(),
            timeout_secs: 300,
            tmpdir: "/tmp/ozzy".to_string(),
            tmpfs_size: "512m".to_string(),
        };
        let fly_config = crate::config::FlyConfig {
            api_token: "fly_test".into(),
            app_name: "test-app".into(),
            api_url: "https://api.machines.dev".into(),
            region: "fra".into(),
            cpu_kind: "shared".into(),
            cpus: 1,
            memory_mb: 512,
        };
        let backend =
            BackendSelector::from_config(&compute_config, Some(&fly_config), None);
        assert!(backend.is_none());
    }
}
