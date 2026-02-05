//! Cache configuration types.

use serde::{Deserialize, Serialize};

/// Configuration for remote cache (S3-compatible storage).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RemoteCacheConfig {
    /// Whether remote cache is enabled
    #[serde(default)]
    pub enabled: bool,

    /// S3 bucket name
    #[serde(default)]
    pub bucket: String,

    /// AWS region (e.g., "us-west-2")
    #[serde(default)]
    pub region: String,

    /// Custom S3 endpoint for S3-compatible services (MinIO, R2, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,

    /// AWS profile to use for credentials
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aws_profile: Option<String>,

    /// Key prefix within the bucket (default: "ozzy-cache/v1")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,

    /// Remote cache policy settings
    #[serde(default)]
    pub policy: RemoteCachePolicy,
}

impl RemoteCacheConfig {
    /// Get the effective prefix (default: "ozzy-cache/v1")
    pub fn effective_prefix(&self) -> &str {
        self.prefix.as_deref().unwrap_or("ozzy-cache/v1")
    }

    /// Check if the configuration is valid for connecting
    pub fn is_configured(&self) -> bool {
        self.enabled && !self.bucket.is_empty() && !self.region.is_empty()
    }

    /// Load from environment variables (overrides config file values)
    pub fn with_env_overrides(mut self) -> Self {
        if let Ok(bucket) = std::env::var("OZZY_CACHE_BUCKET") {
            self.bucket = bucket;
        }
        if let Ok(region) = std::env::var("OZZY_S3_REGION") {
            self.region = region;
        }
        if let Ok(endpoint) = std::env::var("OZZY_S3_ENDPOINT") {
            self.endpoint = Some(endpoint);
        }
        if let Ok(profile) = std::env::var("OZZY_AWS_PROFILE") {
            self.aws_profile = Some(profile);
        }
        if let Ok(enabled) = std::env::var("OZZY_REMOTE_CACHE_ENABLED") {
            self.enabled = enabled.to_lowercase() == "true" || enabled == "1";
        }
        self
    }
}

/// Policy settings for remote cache behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteCachePolicy {
    /// Automatically pull from remote on local cache miss
    #[serde(default)]
    pub auto_pull: bool,

    /// Automatically push to remote after local cache writes
    #[serde(default)]
    pub auto_push: bool,

    /// Fail execution if remote cache operations fail (default: false)
    #[serde(default)]
    pub fail_on_remote_error: bool,

    /// Maximum file size to push to remote (MB, default: 1024)
    #[serde(default = "default_max_push_size_mb")]
    pub max_push_size_mb: u64,
}

fn default_max_push_size_mb() -> u64 {
    1024
}

impl Default for RemoteCachePolicy {
    fn default() -> Self {
        Self {
            auto_pull: true,
            auto_push: false,
            fail_on_remote_error: false,
            max_push_size_mb: default_max_push_size_mb(),
        }
    }
}

/// Configuration for tiered cache behavior.
#[derive(Debug, Clone)]
pub struct TieredCacheConfig {
    /// Remote cache configuration (None = local only)
    pub remote: Option<RemoteCacheConfig>,

    /// Maximum local cache size in bytes (None = unlimited)
    pub max_local_size_bytes: Option<u64>,

    /// Whether to auto-evict when local cache exceeds max size
    pub auto_evict: bool,
}

impl Default for TieredCacheConfig {
    fn default() -> Self {
        Self {
            remote: None,
            max_local_size_bytes: None,
            auto_evict: true,
        }
    }
}
