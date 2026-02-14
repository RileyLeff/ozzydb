//! Compute backend — pluggable execution engine for transform containers.
//!
//! The `ComputeBackend` trait allows swapping Docker (local) for Fly Machines
//! (cloud) or other providers. `ComputeRegistry` holds all active providers
//! and resolves machine names to backends.

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

use std::collections::HashMap;
use std::sync::Arc;

/// Registry of active compute providers.
///
/// Holds named providers (e.g., "docker", "fly") and an optional default.
/// The orchestrator calls `resolve(machine)` to get the backend for each node.
#[derive(Clone)]
pub struct ComputeRegistry {
    providers: HashMap<String, Arc<dyn ComputeBackend>>,
    default_provider: Option<String>,
}

impl ComputeRegistry {
    /// Create a registry from server configuration.
    ///
    /// Registers all enabled providers. If no providers are configured,
    /// the registry is empty (compute is effectively disabled).
    pub fn from_config(
        compute_config: &crate::config::ComputeConfig,
        docker_config: Option<&crate::config::DockerProviderConfig>,
        fly_config: Option<&crate::config::FlyConfig>,
    ) -> Self {
        let mut providers: HashMap<String, Arc<dyn ComputeBackend>> = HashMap::new();

        // Register Docker provider (from_env_optional returns None when disabled)
        if let Some(docker) = docker_config {
            providers.insert(
                "docker".to_string(),
                Arc::new(docker::DockerBackend::new(docker.clone())),
            );
        }

        // Register Fly provider
        if let Some(fly) = fly_config {
            providers.insert(
                "fly".to_string(),
                Arc::new(fly::FlyBackend::new(fly.clone())),
            );
        }

        // Determine default: explicit config > fly (if available) > docker (if available)
        let default_provider = compute_config
            .default_provider
            .clone()
            .or_else(|| {
                if providers.contains_key("fly") {
                    Some("fly".to_string())
                } else if providers.contains_key("docker") {
                    Some("docker".to_string())
                } else {
                    None
                }
            })
            .filter(|name| providers.contains_key(name));

        Self {
            providers,
            default_provider,
        }
    }

    /// Resolve a machine name to a compute backend.
    ///
    /// - `Some(name)`: look up the named provider
    /// - `None`: use the default provider
    ///
    /// Returns an error if the provider isn't registered or no default is set.
    pub fn resolve(&self, machine: Option<&str>) -> anyhow::Result<Arc<dyn ComputeBackend>> {
        match machine {
            Some(name) => self.providers.get(name).cloned().ok_or_else(|| {
                anyhow::anyhow!(
                    "Compute provider '{}' is not available on this server. Available: {}",
                    name,
                    self.provider_names().join(", ")
                )
            }),
            None => {
                let default = self.default_provider.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("No compute providers are configured on this server")
                })?;
                self.providers.get(default).cloned().ok_or_else(|| {
                    anyhow::anyhow!(
                        "Default compute provider '{}' is not registered",
                        default
                    )
                })
            }
        }
    }

    /// List active provider names.
    pub fn provider_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.providers.keys().cloned().collect();
        names.sort();
        names
    }

    /// Get the default provider name (if any).
    pub fn default_provider(&self) -> Option<&str> {
        self.default_provider.as_deref()
    }

    /// Whether any compute providers are registered.
    pub fn is_enabled(&self) -> bool {
        !self.providers.is_empty()
    }

    /// Get the Fly backend (for orphan cleanup task).
    pub fn fly_backend(&self) -> Option<&fly::FlyBackend> {
        self.providers.get("fly").and_then(|b| {
            // Downcast Arc<dyn ComputeBackend> to &FlyBackend
            (b.as_ref() as &dyn std::any::Any).downcast_ref::<fly::FlyBackend>()
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::config::{ComputeConfig, DockerProviderConfig};

    use super::*;

    fn test_compute_config() -> ComputeConfig {
        ComputeConfig {
            timeout_secs: 300,
            tmpdir: "/tmp/ozzy".to_string(),
            default_provider: None,
        }
    }

    fn test_docker_config() -> DockerProviderConfig {
        DockerProviderConfig {
            enabled: true,
            runtime: None,
            memory_limit: "2g".to_string(),
            cpu_limit: "1".to_string(),
        }
    }

    #[test]
    fn test_registry_disabled() {
        let config = test_compute_config();
        let registry = ComputeRegistry::from_config(&config, None, None);
        assert!(!registry.is_enabled());
        assert!(registry.resolve(None).is_err());
    }

    #[test]
    fn test_registry_docker_enabled() {
        let config = test_compute_config();
        let docker = test_docker_config();
        let registry = ComputeRegistry::from_config(&config, Some(&docker), None);
        assert!(registry.is_enabled());
        assert_eq!(registry.default_provider(), Some("docker"));
        assert!(registry.resolve(None).is_ok());
        assert!(registry.resolve(Some("docker")).is_ok());
        assert!(registry.resolve(Some("fly")).is_err());
    }

    #[test]
    fn test_registry_unknown_provider() {
        let config = test_compute_config();
        let docker = test_docker_config();
        let registry = ComputeRegistry::from_config(&config, Some(&docker), None);
        let err = registry.resolve(Some("gpu")).err().expect("should be an error");
        assert!(err.to_string().contains("not available"));
    }
}
