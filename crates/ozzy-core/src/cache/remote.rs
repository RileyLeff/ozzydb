//! Remote cache implementation for S3-compatible storage.
//!
//! Uses the `object_store` crate for S3 operations, which supports:
//! - AWS S3
//! - MinIO
//! - Cloudflare R2
//! - Other S3-compatible services

use bytes::Bytes;
use chrono::Utc;
use futures::TryStreamExt;
use object_store::aws::AmazonS3Builder;
use object_store::path::Path as ObjectPath;
use object_store::{ObjectStore, PutPayload};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;

use super::backend::{CacheEntry, CacheLocation};
use super::config::RemoteCacheConfig;
use crate::error::{Error, Result};
use crate::platform::PlatformFingerprint;

/// Metadata stored alongside each cached parquet file in S3.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntryMeta {
    pub materialized_hash: String,
    pub platform: String,
    pub row_count: Option<u64>,
    pub byte_size: Option<u64>,
    pub created_at: chrono::DateTime<Utc>,
}

/// Remote cache using S3-compatible object storage.
pub struct RemoteCache {
    store: Arc<dyn ObjectStore>,
    prefix: String,
    platform: String,
}

impl RemoteCache {
    /// Create a new remote cache from configuration.
    pub async fn new(config: &RemoteCacheConfig) -> Result<Self> {
        if !config.is_configured() {
            return Err(Error::RemoteCacheNotConfigured);
        }

        let mut builder = AmazonS3Builder::new()
            .with_bucket_name(&config.bucket)
            .with_region(&config.region);

        // Apply credentials from environment if available
        if let Ok(key_id) =
            std::env::var("OZZY_S3_ACCESS_KEY_ID").or_else(|_| std::env::var("AWS_ACCESS_KEY_ID"))
        {
            builder = builder.with_access_key_id(key_id);
        }

        if let Ok(secret) = std::env::var("OZZY_S3_SECRET_ACCESS_KEY")
            .or_else(|_| std::env::var("AWS_SECRET_ACCESS_KEY"))
        {
            builder = builder.with_secret_access_key(secret);
        }

        // Support custom endpoint for S3-compatible services
        if let Some(endpoint) = &config.endpoint {
            builder = builder.with_endpoint(endpoint);
            // Allow HTTP for local development (MinIO)
            if endpoint.starts_with("http://") {
                builder = builder.with_allow_http(true);
            }
        }

        let store = builder
            .build()
            .map_err(|e| Error::RemoteCache(format!("Failed to create S3 client: {}", e)))?;

        let platform = PlatformFingerprint::detect().short_string();
        let prefix = config.effective_prefix().to_string();

        Ok(Self {
            store: Arc::new(store),
            prefix,
            platform,
        })
    }

    /// Get the S3 path for a parquet file.
    fn data_path(&self, hash: &str) -> ObjectPath {
        ObjectPath::from(format!(
            "{}/{}/{}.parquet",
            self.prefix, self.platform, hash
        ))
    }

    /// Get the S3 path for a metadata file.
    fn meta_path(&self, hash: &str) -> ObjectPath {
        ObjectPath::from(format!(
            "{}/{}/{}.meta.json",
            self.prefix, self.platform, hash
        ))
    }

    /// Get the S3 path for a specific platform.
    fn data_path_for_platform(&self, hash: &str, platform: &str) -> ObjectPath {
        ObjectPath::from(format!("{}/{}/{}.parquet", self.prefix, platform, hash))
    }

    /// Get the S3 path for metadata for a specific platform.
    fn meta_path_for_platform(&self, hash: &str, platform: &str) -> ObjectPath {
        ObjectPath::from(format!("{}/{}/{}.meta.json", self.prefix, platform, hash))
    }

    /// Check if a hash exists in the remote cache.
    pub async fn contains(&self, hash: &str) -> Result<bool> {
        let path = self.meta_path(hash);
        match self.store.head(&path).await {
            Ok(_) => Ok(true),
            Err(object_store::Error::NotFound { .. }) => Ok(false),
            Err(e) => Err(Error::RemoteCache(format!("HEAD request failed: {}", e))),
        }
    }

    /// Check if a hash exists for a specific platform.
    pub async fn contains_for_platform(&self, hash: &str, platform: &str) -> Result<bool> {
        let path = self.meta_path_for_platform(hash, platform);
        match self.store.head(&path).await {
            Ok(_) => Ok(true),
            Err(object_store::Error::NotFound { .. }) => Ok(false),
            Err(e) => Err(Error::RemoteCache(format!("HEAD request failed: {}", e))),
        }
    }

    /// Get cache entry metadata.
    pub async fn get_meta(&self, hash: &str) -> Result<Option<CacheEntryMeta>> {
        let path = self.meta_path(hash);
        match self.store.get(&path).await {
            Ok(result) => {
                let bytes = result
                    .bytes()
                    .await
                    .map_err(|e| Error::RemoteCache(format!("Failed to read metadata: {}", e)))?;
                let meta: CacheEntryMeta = serde_json::from_slice(&bytes)?;
                Ok(Some(meta))
            }
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(e) => Err(Error::RemoteCache(format!("GET metadata failed: {}", e))),
        }
    }

    /// Download a cached file to a local path.
    /// Returns true if the file was downloaded, false if not found.
    pub async fn download(&self, hash: &str, dest: &Path) -> Result<bool> {
        let path = self.data_path(hash);

        match self.store.get(&path).await {
            Ok(result) => {
                // Stream the data to the destination file
                let bytes = result
                    .bytes()
                    .await
                    .map_err(|e| Error::RemoteCache(format!("Failed to download file: {}", e)))?;

                tokio::fs::write(dest, &bytes).await.map_err(|e| {
                    Error::RemoteCache(format!("Failed to write downloaded file: {}", e))
                })?;

                Ok(true)
            }
            Err(object_store::Error::NotFound { .. }) => Ok(false),
            Err(e) => Err(Error::RemoteCache(format!("GET data failed: {}", e))),
        }
    }

    /// Download a cached file for a specific platform.
    pub async fn download_for_platform(
        &self,
        hash: &str,
        platform: &str,
        dest: &Path,
    ) -> Result<bool> {
        let path = self.data_path_for_platform(hash, platform);

        match self.store.get(&path).await {
            Ok(result) => {
                let bytes = result
                    .bytes()
                    .await
                    .map_err(|e| Error::RemoteCache(format!("Failed to download file: {}", e)))?;

                tokio::fs::write(dest, &bytes).await.map_err(|e| {
                    Error::RemoteCache(format!("Failed to write downloaded file: {}", e))
                })?;

                Ok(true)
            }
            Err(object_store::Error::NotFound { .. }) => Ok(false),
            Err(e) => Err(Error::RemoteCache(format!("GET data failed: {}", e))),
        }
    }

    /// Upload a file to the remote cache.
    pub async fn upload(
        &self,
        hash: &str,
        platform: &str,
        source_path: &Path,
        row_count: Option<u64>,
    ) -> Result<()> {
        // Read the source file
        let bytes = tokio::fs::read(source_path)
            .await
            .map_err(|e| Error::RemoteCache(format!("Failed to read source file: {}", e)))?;
        let byte_size = bytes.len() as u64;

        // Upload the parquet file
        let data_path = self.data_path_for_platform(hash, platform);
        self.store
            .put(&data_path, PutPayload::from(Bytes::from(bytes)))
            .await
            .map_err(|e| Error::RemoteCache(format!("Failed to upload data: {}", e)))?;

        // Create and upload metadata
        let meta = CacheEntryMeta {
            materialized_hash: hash.to_string(),
            platform: platform.to_string(),
            row_count,
            byte_size: Some(byte_size),
            created_at: Utc::now(),
        };
        let meta_bytes = serde_json::to_vec(&meta)?;
        let meta_path = self.meta_path_for_platform(hash, platform);
        self.store
            .put(&meta_path, PutPayload::from(Bytes::from(meta_bytes)))
            .await
            .map_err(|e| Error::RemoteCache(format!("Failed to upload metadata: {}", e)))?;

        Ok(())
    }

    /// Delete an entry from the remote cache.
    pub async fn delete(&self, hash: &str) -> Result<()> {
        let data_path = self.data_path(hash);
        let meta_path = self.meta_path(hash);

        // Delete both files, ignoring NotFound errors
        match self.store.delete(&data_path).await {
            Ok(()) => {}
            Err(object_store::Error::NotFound { .. }) => {}
            Err(e) => return Err(Error::RemoteCache(format!("Failed to delete data: {}", e))),
        }

        match self.store.delete(&meta_path).await {
            Ok(()) => {}
            Err(object_store::Error::NotFound { .. }) => {}
            Err(e) => {
                return Err(Error::RemoteCache(format!(
                    "Failed to delete metadata: {}",
                    e
                )));
            }
        }

        Ok(())
    }

    /// List all cache entries for the current platform.
    pub async fn list(&self) -> Result<Vec<CacheEntry>> {
        self.list_for_platform(&self.platform).await
    }

    /// List all cache entries for a specific platform.
    pub async fn list_for_platform(&self, platform: &str) -> Result<Vec<CacheEntry>> {
        let prefix = ObjectPath::from(format!("{}/{}/", self.prefix, platform));

        let entries: Vec<_> = self
            .store
            .list(Some(&prefix))
            .try_collect()
            .await
            .map_err(|e| Error::RemoteCache(format!("Failed to list objects: {}", e)))?;

        let mut cache_entries = Vec::new();

        // Find all .meta.json files and parse them
        for entry in entries {
            let key = entry.location.to_string();
            if key.ends_with(".meta.json") {
                // Extract hash from path
                let hash = key
                    .trim_end_matches(".meta.json")
                    .rsplit('/')
                    .next()
                    .unwrap_or_default();

                if let Some(meta) = self.get_meta(hash).await? {
                    cache_entries.push(CacheEntry {
                        materialized_hash: meta.materialized_hash,
                        platform: meta.platform.clone(),
                        row_count: meta.row_count,
                        byte_size: meta.byte_size,
                        created_at: meta.created_at,
                        last_accessed: meta.created_at, // Remote doesn't track access
                        access_count: 0,                // Remote doesn't track access count
                        location: CacheLocation::Remote {
                            bucket: self.prefix.clone(),
                            key: format!("{}/{}/{}.parquet", self.prefix, meta.platform, hash),
                        },
                    });
                }
            }
        }

        Ok(cache_entries)
    }

    /// List all platforms that have cache entries.
    pub async fn list_platforms(&self) -> Result<Vec<String>> {
        let prefix = ObjectPath::from(format!("{}/", self.prefix));

        let entries: Vec<_> = self
            .store
            .list_with_delimiter(Some(&prefix))
            .await
            .map_err(|e| Error::RemoteCache(format!("Failed to list prefixes: {}", e)))?
            .common_prefixes;

        let platforms: Vec<String> = entries
            .into_iter()
            .map(|p| {
                p.to_string()
                    .trim_start_matches(&format!("{}/", self.prefix))
                    .trim_end_matches('/')
                    .to_string()
            })
            .filter(|s| !s.is_empty())
            .collect();

        Ok(platforms)
    }

    /// Get the current platform string.
    pub fn platform(&self) -> &str {
        &self.platform
    }

    /// Get the S3 prefix.
    pub fn prefix(&self) -> &str {
        &self.prefix
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_remote_cache_paths() {
        // Test path generation
        let prefix = "ozzy-cache/v1";
        let platform = "aarch64-macos";
        let hash = "abc123def456";

        let data_path = format!("{}/{}/{}.parquet", prefix, platform, hash);
        let meta_path = format!("{}/{}/{}.meta.json", prefix, platform, hash);

        assert_eq!(
            data_path,
            "ozzy-cache/v1/aarch64-macos/abc123def456.parquet"
        );
        assert_eq!(
            meta_path,
            "ozzy-cache/v1/aarch64-macos/abc123def456.meta.json"
        );
    }

    #[tokio::test]
    async fn test_cache_entry_meta_serialization() {
        let meta = CacheEntryMeta {
            materialized_hash: "abc123".to_string(),
            platform: "aarch64-macos".to_string(),
            row_count: Some(1000),
            byte_size: Some(4096),
            created_at: Utc::now(),
        };

        let json = serde_json::to_string(&meta).unwrap();
        let parsed: CacheEntryMeta = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.materialized_hash, "abc123");
        assert_eq!(parsed.platform, "aarch64-macos");
        assert_eq!(parsed.row_count, Some(1000));
        assert_eq!(parsed.byte_size, Some(4096));
    }
}
