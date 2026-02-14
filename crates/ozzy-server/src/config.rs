//! Server configuration loaded from environment variables.

use anyhow::{Context, Result};

/// Parse an optional hex-encoded key of expected byte length.
fn parse_hex_key(var_name: &str, expected_bytes: usize) -> Result<Option<Vec<u8>>> {
    let hex_str = match std::env::var(var_name).ok().filter(|s| !s.is_empty()) {
        Some(s) => s,
        None => return Ok(None),
    };

    if hex_str.len() != expected_bytes * 2 {
        anyhow::bail!(
            "{} must be {} hex characters ({} bytes), got {}",
            var_name,
            expected_bytes * 2,
            expected_bytes,
            hex_str.len()
        );
    }

    let bytes: Result<Vec<u8>, _> = (0..hex_str.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex_str[i..i + 2], 16))
        .collect();

    Ok(Some(bytes.map_err(|e| {
        anyhow::anyhow!("{} contains invalid hex: {}", var_name, e)
    })?))
}

/// Server configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// Address to bind to (e.g., "0.0.0.0:3000")
    pub bind_address: String,

    /// PostgreSQL connection URL
    pub database_url: String,

    /// Maximum database connections
    pub db_max_connections: u32,

    /// GitHub OAuth client ID
    pub github_client_id: String,

    /// GitHub OAuth client secret
    pub github_client_secret: String,

    /// Base URL for this server (for OAuth callbacks)
    pub base_url: String,

    /// Local cache directory for read-through caching of R2 content.
    pub cache_dir: String,

    /// Optional R2/S3 storage backend (primary when configured).
    pub r2: Option<R2Config>,

    /// Maximum multipart upload size in bytes (default: 1GB)
    pub max_upload_size_bytes: u64,

    /// Allowed CORS origins (comma-separated, or "*" for any)
    pub cors_origins: String,

    /// Optional allowlist of GitHub usernames permitted to log in.
    /// When empty, registration is open to everyone.
    /// Set via ALLOWED_LOGINS=rileyleff,collaborator2
    pub allowed_logins: Vec<String>,

    /// AES-256-GCM key for encrypting project secrets (hex-encoded, 32 bytes = 64 hex chars).
    /// Required for the secrets API. Set via SECRETS_ENCRYPTION_KEY env var.
    pub secrets_encryption_key: Option<Vec<u8>>,

    /// GitHub App configuration (optional). When set, enables private repo access
    /// via GitHub App installation tokens. Without this, only public repos are accessible.
    pub github_app: Option<GitHubAppConfig>,

    /// Compute configuration for environment building and transform execution.
    pub compute: ComputeConfig,

    /// Fly Machines configuration (optional). When set, compute dispatches to Fly.
    pub fly: Option<FlyConfig>,

    /// Rate limiting configuration for compute jobs.
    pub rate_limit: RateLimitConfig,

    /// Dev mode: auto-create a user with this username on startup
    /// and print an auth token. Set via DEV_AUTO_USER env var.
    pub dev_auto_user: Option<String>,
}

/// Compute configuration for Docker-based environment building and execution.
#[derive(Debug, Clone)]
pub struct ComputeConfig {
    /// Whether compute is enabled on this server.
    pub enabled: bool,
    /// Docker runtime to use for compute containers (e.g., "runsc" for gVisor).
    pub docker_runtime: Option<String>,
    /// Memory limit for compute containers (e.g., "2g").
    pub memory_limit: String,
    /// CPU limit for compute containers (e.g., "1").
    pub cpu_limit: String,
    /// Timeout in seconds for compute jobs (default: 300).
    pub timeout_secs: u64,
    /// Temporary directory for compute workspaces.
    pub tmpdir: String,
    /// tmpfs size for compute containers (e.g., "512m").
    pub tmpfs_size: String,
}

/// GitHub App configuration for repository access.
#[derive(Debug, Clone)]
pub struct GitHubAppConfig {
    /// GitHub App ID
    pub app_id: u64,
    /// PEM-encoded RSA private key for JWT signing
    pub private_key: String,
    /// Webhook secret for verifying GitHub webhook payloads (optional)
    pub webhook_secret: Option<String>,
}

/// R2/S3 storage configuration.
#[derive(Debug, Clone)]
pub struct R2Config {
    /// S3-compatible endpoint URL (e.g., https://xxx.r2.cloudflarestorage.com)
    pub endpoint: String,

    /// Bucket name
    pub bucket: String,

    /// Access key ID
    pub access_key_id: String,

    /// Secret access key
    pub secret_access_key: String,

    /// Region (defaults to "auto" for R2)
    pub region: String,
}

/// Fly Machines configuration for remote compute.
#[derive(Debug, Clone)]
pub struct FlyConfig {
    /// Fly API token (Bearer token for Machines API).
    pub api_token: String,
    /// Fly app name where machines are created.
    pub app_name: String,
    /// Fly Machines API base URL.
    pub api_url: String,
    /// Default region for machine creation (e.g., "fra").
    pub region: String,
    /// Default CPU kind (e.g., "shared").
    pub cpu_kind: String,
    /// Default CPU count.
    pub cpus: u32,
    /// Default memory in MB.
    pub memory_mb: u32,
}

/// Rate limiting configuration for compute jobs.
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Maximum concurrent jobs across all users (0 = unlimited).
    pub global_max_concurrent: u32,
    /// Maximum concurrent jobs per user (0 = unlimited).
    pub per_user_max_concurrent: u32,
}

impl Config {
    /// Load configuration from environment variables.
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            bind_address: std::env::var("BIND_ADDRESS").unwrap_or_else(|_| "0.0.0.0:3000".into()),
            database_url: std::env::var("DATABASE_URL")
                .context("DATABASE_URL environment variable required")?,
            db_max_connections: std::env::var("DB_MAX_CONNECTIONS")
                .unwrap_or_else(|_| "10".into())
                .parse()
                .context("DB_MAX_CONNECTIONS must be a number")?,
            github_client_id: std::env::var("GITHUB_CLIENT_ID")
                .context("GITHUB_CLIENT_ID environment variable required")?,
            github_client_secret: std::env::var("GITHUB_CLIENT_SECRET")
                .context("GITHUB_CLIENT_SECRET environment variable required")?,
            base_url: std::env::var("BASE_URL").unwrap_or_else(|_| "http://localhost:3000".into()),
            cache_dir: std::env::var("LOCAL_STORAGE_PATH")
                .unwrap_or_else(|_| "/tmp/ozzydb-content".into()),
            r2: R2Config::from_env_optional(),
            max_upload_size_bytes: std::env::var("MAX_UPLOAD_SIZE_BYTES")
                .unwrap_or_else(|_| "1073741824".into()) // 1GB default
                .parse()
                .context("MAX_UPLOAD_SIZE_BYTES must be a number")?,
            cors_origins: std::env::var("CORS_ORIGINS").unwrap_or_else(|_| "*".into()),
            allowed_logins: std::env::var("ALLOWED_LOGINS")
                .unwrap_or_default()
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            secrets_encryption_key: parse_hex_key("SECRETS_ENCRYPTION_KEY", 32)?,
            github_app: GitHubAppConfig::from_env_optional(),
            compute: ComputeConfig::from_env(),
            fly: FlyConfig::from_env_optional(),
            rate_limit: RateLimitConfig::from_env(),
            dev_auto_user: std::env::var("DEV_AUTO_USER")
                .ok()
                .filter(|s| !s.is_empty()),
        })
    }
}

impl ComputeConfig {
    /// Load compute configuration from environment variables.
    pub fn from_env() -> Self {
        Self {
            enabled: std::env::var("COMPUTE_ENABLED")
                .unwrap_or_else(|_| "false".into())
                .parse()
                .unwrap_or(false),
            docker_runtime: std::env::var("COMPUTE_DOCKER_RUNTIME")
                .ok()
                .filter(|s| !s.is_empty()),
            memory_limit: std::env::var("COMPUTE_MEMORY_LIMIT").unwrap_or_else(|_| "2g".into()),
            cpu_limit: std::env::var("COMPUTE_CPU_LIMIT").unwrap_or_else(|_| "1".into()),
            timeout_secs: std::env::var("COMPUTE_TIMEOUT_SECS")
                .unwrap_or_else(|_| "300".into())
                .parse()
                .unwrap_or(300),
            tmpdir: std::env::var("COMPUTE_TMPDIR")
                .or_else(|_| std::env::var("TMPDIR"))
                .unwrap_or_else(|_| "/tmp/ozzy".into()),
            tmpfs_size: std::env::var("COMPUTE_TMPFS_SIZE").unwrap_or_else(|_| "512m".into()),
        }
    }
}

impl GitHubAppConfig {
    /// Load optional GitHub App configuration from environment variables.
    /// Returns None when GITHUB_APP_ID or GITHUB_APP_PRIVATE_KEY is not set.
    pub fn from_env_optional() -> Option<Self> {
        let app_id: u64 = std::env::var("GITHUB_APP_ID")
            .ok()
            .filter(|s| !s.is_empty())?
            .parse()
            .ok()?;
        let private_key = std::env::var("GITHUB_APP_PRIVATE_KEY")
            .ok()
            .filter(|s| !s.is_empty())?;
        let webhook_secret = std::env::var("GITHUB_WEBHOOK_SECRET")
            .ok()
            .filter(|s| !s.is_empty());

        Some(Self {
            app_id,
            private_key,
            webhook_secret,
        })
    }
}

impl FlyConfig {
    /// Load optional Fly configuration from environment variables.
    /// Returns None when FLY_API_TOKEN is not set.
    pub fn from_env_optional() -> Option<Self> {
        let api_token = std::env::var("FLY_API_TOKEN")
            .ok()
            .filter(|s| !s.is_empty())?;
        let app_name = std::env::var("FLY_APP_NAME")
            .ok()
            .filter(|s| !s.is_empty())?;

        Some(Self {
            api_token,
            app_name,
            api_url: std::env::var("FLY_API_URL")
                .unwrap_or_else(|_| "https://api.machines.dev".into()),
            region: std::env::var("FLY_REGION").unwrap_or_else(|_| "fra".into()),
            cpu_kind: std::env::var("FLY_CPU_KIND").unwrap_or_else(|_| "shared".into()),
            cpus: std::env::var("FLY_CPUS")
                .unwrap_or_else(|_| "1".into())
                .parse()
                .unwrap_or(1),
            memory_mb: std::env::var("FLY_MEMORY_MB")
                .unwrap_or_else(|_| "512".into())
                .parse()
                .unwrap_or(512),
        })
    }
}

impl RateLimitConfig {
    /// Load rate limit configuration from environment variables.
    pub fn from_env() -> Self {
        Self {
            global_max_concurrent: std::env::var("RATE_LIMIT_GLOBAL_MAX")
                .unwrap_or_else(|_| "20".into())
                .parse()
                .unwrap_or(20),
            per_user_max_concurrent: std::env::var("RATE_LIMIT_PER_USER_MAX")
                .unwrap_or_else(|_| "5".into())
                .parse()
                .unwrap_or(5),
        }
    }
}

impl R2Config {
    /// Load R2 configuration from environment variables.
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            endpoint: std::env::var("R2_ENDPOINT")
                .context("R2_ENDPOINT environment variable required")?,
            bucket: std::env::var("R2_BUCKET")
                .context("R2_BUCKET environment variable required")?,
            access_key_id: std::env::var("R2_ACCESS_KEY_ID")
                .context("R2_ACCESS_KEY_ID environment variable required")?,
            secret_access_key: std::env::var("R2_SECRET_ACCESS_KEY")
                .context("R2_SECRET_ACCESS_KEY environment variable required")?,
            region: std::env::var("R2_REGION").unwrap_or_else(|_| "auto".into()),
        })
    }

    /// Load optional R2 configuration.
    /// Returns None when required R2 credentials are not all present (or empty).
    pub fn from_env_optional() -> Option<Self> {
        let endpoint = std::env::var("R2_ENDPOINT")
            .ok()
            .filter(|s| !s.is_empty())?;
        let bucket = std::env::var("R2_BUCKET").ok().filter(|s| !s.is_empty())?;
        let access_key_id = std::env::var("R2_ACCESS_KEY_ID")
            .ok()
            .filter(|s| !s.is_empty())?;
        let secret_access_key = std::env::var("R2_SECRET_ACCESS_KEY")
            .ok()
            .filter(|s| !s.is_empty())?;

        Some(Self {
            endpoint,
            bucket,
            access_key_id,
            secret_access_key,
            region: std::env::var("R2_REGION").unwrap_or_else(|_| "auto".into()),
        })
    }
}
