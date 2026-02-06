//! Server configuration loaded from environment variables.

use anyhow::{Context, Result};

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

    /// Local filesystem storage root (NVMe primary).
    pub local_storage_path: String,

    /// Optional R2/S3 redundancy backend.
    pub r2: Option<R2Config>,

    /// Maximum tar archive size in bytes (default: 1GB)
    pub max_tar_size_bytes: u64,

    /// Maximum multipart upload size in bytes (default: 100MB)
    pub max_upload_size_bytes: u64,

    /// Allowed CORS origins (comma-separated, or "*" for any)
    pub cors_origins: String,
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
            local_storage_path: std::env::var("LOCAL_STORAGE_PATH")
                .unwrap_or_else(|_| "/tmp/ozzydb-content".into()),
            r2: R2Config::from_env_optional(),
            max_tar_size_bytes: std::env::var("MAX_TAR_SIZE_BYTES")
                .unwrap_or_else(|_| "1073741824".into()) // 1GB default
                .parse()
                .context("MAX_TAR_SIZE_BYTES must be a number")?,
            max_upload_size_bytes: std::env::var("MAX_UPLOAD_SIZE_BYTES")
                .unwrap_or_else(|_| "104857600".into()) // 100MB default
                .parse()
                .context("MAX_UPLOAD_SIZE_BYTES must be a number")?,
            cors_origins: std::env::var("CORS_ORIGINS").unwrap_or_else(|_| "*".into()),
        })
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
    /// Returns None when required R2 credentials are not all present.
    pub fn from_env_optional() -> Option<Self> {
        let endpoint = std::env::var("R2_ENDPOINT").ok()?;
        let bucket = std::env::var("R2_BUCKET").ok()?;
        let access_key_id = std::env::var("R2_ACCESS_KEY_ID").ok()?;
        let secret_access_key = std::env::var("R2_SECRET_ACCESS_KEY").ok()?;

        Some(Self {
            endpoint,
            bucket,
            access_key_id,
            secret_access_key,
            region: std::env::var("R2_REGION").unwrap_or_else(|_| "auto".into()),
        })
    }
}
