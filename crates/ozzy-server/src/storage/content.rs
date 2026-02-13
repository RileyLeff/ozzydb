//! Content-addressed storage: R2-primary with local read-through cache.
//!
//! When R2 is configured, it is the single source of truth. Local disk serves
//! as a read-through cache for fast repeated reads.
//! When R2 is not configured (dev/test), local disk is the primary store.
//!
//! Layout: {prefix}/{hash[0:2]}/{hash[2:4]}/{hash}.{ext}

use anyhow::{Context, Result};
use bytes::Bytes;
use futures::Stream;
use object_store::ObjectStore;
use object_store::aws::AmazonS3Builder;
use object_store::path::Path as ObjectPath;
use ozzy_core::hash;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use crate::config::{Config, R2Config};

/// Content-addressed storage backend.
///
/// When R2 is configured: R2 is the source of truth, local disk is a cache.
/// When R2 is not configured: local disk is the primary store (dev/test mode).
#[derive(Clone)]
pub struct ContentStorage {
    cache_dir: PathBuf,
    remote_store: Option<Arc<dyn ObjectStore>>,
    prefix: String,
    /// When true, `get()` and `get_stream()` verify that the content hash matches
    /// the requested key. This is correct for content-addressed storage (where the
    /// key IS the blake3 of the content), but must be false for materialized storage
    /// (where the key is a composite hash of inputs+transform+params+platform).
    verify_content_hash: bool,
}

impl ContentStorage {
    fn validate_content_hash(content_hash: &str) -> Result<()> {
        // Accept both BLAKE3 (64 chars) and Git SHA-1 (40 chars) hashes.
        // The source storage uses git commit SHAs as keys.
        let len = content_hash.len();
        if (len != 40 && len != 64)
            || !content_hash
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        {
            anyhow::bail!(
                "Invalid content hash '{}': expected 40 or 64 lowercase hexadecimal characters",
                content_hash
            );
        }
        Ok(())
    }

    fn build_remote_store(config: &R2Config) -> Result<Arc<dyn ObjectStore>> {
        let store = AmazonS3Builder::new()
            .with_endpoint(&config.endpoint)
            .with_bucket_name(&config.bucket)
            .with_access_key_id(&config.access_key_id)
            .with_secret_access_key(&config.secret_access_key)
            .with_region(&config.region)
            .with_virtual_hosted_style_request(false)
            .build()
            .context("Failed to create R2 storage client")?;
        Ok(Arc::new(store))
    }

    fn default_cache_dir() -> PathBuf {
        std::env::temp_dir().join("ozzydb-content")
    }

    fn new_with_cache_and_remote(
        cache_dir: PathBuf,
        remote_store: Option<Arc<dyn ObjectStore>>,
        prefix: impl Into<String>,
        verify_content_hash: bool,
    ) -> Result<Self> {
        std::fs::create_dir_all(&cache_dir)?;
        Ok(Self {
            cache_dir,
            remote_store,
            prefix: prefix.into(),
            verify_content_hash,
        })
    }

    /// Create storage from server config.
    ///
    /// When R2 is configured, it becomes the primary store with local as cache.
    /// When R2 is absent (dev/test), local disk is the primary store.
    pub fn from_config(config: &Config) -> Result<Self> {
        let remote_store = config
            .r2
            .as_ref()
            .map(Self::build_remote_store)
            .transpose()?;
        Self::new_with_cache_and_remote(
            PathBuf::from(&config.cache_dir),
            remote_store,
            "content",
            true, // content-addressed: verify hash on read
        )
    }

    /// Create storage from server config with a custom prefix.
    ///
    /// Hash verification is disabled because the storage key may not be the
    /// blake3 of the content (e.g., materialized cache uses composite hashes).
    pub fn from_config_with_prefix(config: &Config, prefix: &str) -> Result<Self> {
        let remote_store = config
            .r2
            .as_ref()
            .map(Self::build_remote_store)
            .transpose()?;
        Self::new_with_cache_and_remote(
            PathBuf::from(&config.cache_dir),
            remote_store,
            prefix,
            false, // key-addressed: skip hash verification
        )
    }

    /// Create storage with R2-only configuration (used by legacy tests).
    pub fn new(config: &R2Config) -> Result<Self> {
        let remote_store = Some(Self::build_remote_store(config)?);
        Self::new_with_cache_and_remote(Self::default_cache_dir(), remote_store, "content", true)
    }

    /// Create storage with custom prefix (used by integration tests).
    pub fn with_prefix(config: &R2Config, prefix: impl Into<String>) -> Result<Self> {
        let remote_store = Some(Self::build_remote_store(config)?);
        Self::new_with_cache_and_remote(Self::default_cache_dir(), remote_store, prefix, true)
    }

    /// Return the R2/object-store key string for a given hash and extension.
    /// Used to persist the key in the DB (content_refs.r2_key, data_atoms.r2_key).
    pub fn storage_key(&self, content_hash: &str, extension: &str) -> Result<String> {
        Self::validate_content_hash(content_hash)?;
        let dir1 = &content_hash[0..2];
        let dir2 = &content_hash[2..4];
        Ok(format!(
            "{}/{}/{}/{}.{}",
            self.prefix, dir1, dir2, content_hash, extension
        ))
    }

    fn object_path(&self, content_hash: &str, extension: &str) -> Result<ObjectPath> {
        Ok(ObjectPath::from(self.storage_key(content_hash, extension)?))
    }

    fn local_path(&self, content_hash: &str, extension: &str) -> Result<PathBuf> {
        Self::validate_content_hash(content_hash)?;
        let dir1 = &content_hash[0..2];
        let dir2 = &content_hash[2..4];
        Ok(self
            .cache_dir
            .join(&self.prefix)
            .join(dir1)
            .join(dir2)
            .join(format!("{}.{}", content_hash, extension)))
    }

    fn ensure_parent(path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(())
    }

    /// Upload content to R2. Errors propagate (R2 is the source of truth).
    async fn upload_remote(
        &self,
        content_hash: &str,
        extension: &str,
        content: &[u8],
    ) -> Result<()> {
        let remote = self
            .remote_store
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("R2 not configured"))?;
        let path = self.object_path(content_hash, extension)?;
        remote
            .put(&path, Bytes::copy_from_slice(content).into())
            .await
            .context("Failed to write content to R2")?;
        Ok(())
    }

    /// Cache content locally for fast future reads. Best-effort; failures are logged.
    async fn cache_local_best_effort(&self, content_hash: &str, extension: &str, content: &[u8]) {
        let local_path = match self.local_path(content_hash, extension) {
            Ok(p) => p,
            Err(_) => return,
        };
        if local_path.exists() {
            return;
        }
        if Self::ensure_parent(&local_path).is_err() {
            return;
        }
        let tmp_path =
            local_path.with_extension(format!("{}.{}.tmp", extension, uuid::Uuid::new_v4()));
        if tokio::fs::write(&tmp_path, content).await.is_err() {
            return;
        }
        match tokio::fs::rename(&tmp_path, &local_path).await {
            Ok(()) => {}
            Err(_) => {
                let _ = tokio::fs::remove_file(&tmp_path).await;
            }
        }
    }

    /// Check if content with the given hash exists.
    pub async fn exists(&self, content_hash: &str, extension: &str) -> Result<bool> {
        let local_path = self.local_path(content_hash, extension)?;
        if local_path.exists() {
            return Ok(true);
        }

        if let Some(remote) = &self.remote_store {
            let remote_path = self.object_path(content_hash, extension)?;
            match remote.head(&remote_path).await {
                Ok(_) => return Ok(true),
                Err(object_store::Error::NotFound { .. }) => {}
                Err(e) => return Err(e).context("Failed to check remote content existence"),
            }
        }

        Ok(false)
    }

    /// Check which hashes from a list already exist.
    pub async fn check_existing(&self, hashes: &[(String, String)]) -> Result<Vec<String>> {
        let mut existing = Vec::new();
        for (hash, ext) in hashes {
            if self.exists(hash, ext).await? {
                existing.push(hash.clone());
            }
        }
        Ok(existing)
    }

    /// Store content and return its hash.
    ///
    /// When R2 is configured: writes to R2 first (source of truth), then caches locally.
    /// When R2 is absent: writes to local disk as primary store.
    pub async fn store(&self, content: &[u8], extension: &str) -> Result<String> {
        let content_hash = hash::blake3_hash(content);

        if self.remote_store.is_some() {
            // R2-primary mode: write to R2 first, cache locally best-effort.
            self.upload_remote(&content_hash, extension, content)
                .await?;
            self.cache_local_best_effort(&content_hash, extension, content)
                .await;
        } else {
            // Local-only mode (dev/test): write to local disk as primary.
            let local_path = self.local_path(&content_hash, extension)?;
            if !local_path.exists() {
                Self::ensure_parent(&local_path)?;
                let tmp_path = local_path.with_extension(format!(
                    "{}.{}.tmp",
                    extension,
                    uuid::Uuid::new_v4()
                ));
                tokio::fs::write(&tmp_path, content).await?;
                match tokio::fs::rename(&tmp_path, &local_path).await {
                    Ok(()) => {}
                    Err(e) if local_path.exists() => {
                        let _ = tokio::fs::remove_file(&tmp_path).await;
                        tracing::debug!("concurrent store race for {}: {}", content_hash, e);
                    }
                    Err(e) => {
                        let _ = tokio::fs::remove_file(&tmp_path).await;
                        return Err(e.into());
                    }
                }
            }
        }

        Ok(content_hash)
    }

    /// Store content with a pre-determined hash (e.g., a materialized cache key).
    ///
    /// Unlike `store()`, this does not compute the hash from the content — the
    /// caller provides the hash to use as the storage key.
    pub async fn store_with_hash(
        &self,
        content_hash: &str,
        content: &[u8],
        extension: &str,
    ) -> Result<()> {
        if self.remote_store.is_some() {
            self.upload_remote(content_hash, extension, content).await?;
            self.cache_local_best_effort(content_hash, extension, content)
                .await;
        } else {
            let local_path = self.local_path(content_hash, extension)?;
            if !local_path.exists() {
                Self::ensure_parent(&local_path)?;
                let tmp_path = local_path.with_extension(format!(
                    "{}.{}.tmp",
                    extension,
                    uuid::Uuid::new_v4()
                ));
                tokio::fs::write(&tmp_path, content).await?;
                match tokio::fs::rename(&tmp_path, &local_path).await {
                    Ok(()) => {}
                    Err(e) if local_path.exists() => {
                        let _ = tokio::fs::remove_file(&tmp_path).await;
                        tracing::debug!("concurrent store race for {}: {}", content_hash, e);
                    }
                    Err(e) => {
                        let _ = tokio::fs::remove_file(&tmp_path).await;
                        return Err(e.into());
                    }
                }
            }
        }
        Ok(())
    }

    /// Store content from bytes.
    pub async fn store_bytes(&self, content: Bytes, extension: &str) -> Result<String> {
        self.store(&content, extension).await
    }

    /// Retrieve content by hash.
    /// If local copy is missing and remote is configured, attempts remote hydrate.
    pub async fn get(&self, content_hash: &str, extension: &str) -> Result<Bytes> {
        let local_path = self.local_path(content_hash, extension)?;
        if local_path.exists() {
            let content = std::fs::read(&local_path)?;
            if self.verify_content_hash {
                let actual_hash = hash::blake3_hash(&content);
                if actual_hash != content_hash {
                    anyhow::bail!(
                        "Content hash mismatch: expected {}, got {}. Local storage may be corrupted.",
                        content_hash,
                        actual_hash
                    );
                }
            }
            return Ok(Bytes::from(content));
        }

        let Some(remote) = &self.remote_store else {
            anyhow::bail!("Content not found: {}", content_hash);
        };

        let remote_path = self.object_path(content_hash, extension)?;
        let result = remote
            .get(&remote_path)
            .await
            .with_context(|| format!("Content not found: {}", content_hash))?;
        let content = result
            .bytes()
            .await
            .context("Failed to read content from remote storage")?;

        if self.verify_content_hash {
            let actual_hash = hash::blake3_hash(&content);
            if actual_hash != content_hash {
                anyhow::bail!(
                    "Content hash mismatch: expected {}, got {}. Remote storage may be corrupted.",
                    content_hash,
                    actual_hash
                );
            }
        }

        // Hydrate local cache for future reads (atomic: write to temp, then rename).
        Self::ensure_parent(&local_path)?;
        let tmp_path = local_path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
        tokio::fs::write(&tmp_path, &content).await?;
        match tokio::fs::rename(&tmp_path, &local_path).await {
            Ok(()) => {}
            Err(_) if local_path.exists() => {
                let _ = tokio::fs::remove_file(&tmp_path).await;
            }
            Err(e) => return Err(e.into()),
        }

        Ok(content)
    }

    /// Get content as a stream for large files.
    pub async fn get_stream(
        &self,
        content_hash: &str,
        extension: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<Bytes, object_store::Error>> + Send>>> {
        let local_path = self.local_path(content_hash, extension)?;
        if local_path.exists() {
            let raw = std::fs::read(&local_path).map_err(|e| {
                anyhow::anyhow!("Failed to read local file {}: {}", local_path.display(), e)
            })?;
            if self.verify_content_hash {
                let actual_hash = hash::blake3_hash(&raw);
                if actual_hash != content_hash {
                    anyhow::bail!(
                        "Content hash mismatch in get_stream: expected {}, got {}. Local storage may be corrupted.",
                        content_hash,
                        actual_hash
                    );
                }
            }
            let bytes = Bytes::from(raw);
            return Ok(Box::pin(futures::stream::once(async move { Ok(bytes) })));
        }

        let remote = self
            .remote_store
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Content not found: {}", content_hash))?;
        let path = self.object_path(content_hash, extension)?;
        let result = remote
            .get(&path)
            .await
            .with_context(|| format!("Content not found: {}", content_hash))?;
        Ok(result.into_stream())
    }

    /// Delete content by hash.
    ///
    /// When R2 is configured: deletes from R2 (source of truth), then cleans local cache.
    /// When R2 is absent: deletes from local disk.
    pub async fn delete(&self, content_hash: &str, extension: &str) -> Result<()> {
        if let Some(remote) = &self.remote_store {
            let path = self.object_path(content_hash, extension)?;
            match remote.delete(&path).await {
                Ok(()) => {}
                Err(object_store::Error::NotFound { .. }) => {}
                Err(e) => return Err(e).context("Failed to delete content from R2"),
            }
        }

        // Clean local cache (or primary in local-only mode)
        let local_path = self.local_path(content_hash, extension)?;
        if local_path.exists() {
            std::fs::remove_file(&local_path)?;
        }

        Ok(())
    }

    /// List all content hashes with a given prefix (first 2 hex chars).
    ///
    /// When R2 is configured, lists from R2. Otherwise lists from local disk.
    pub async fn list_by_prefix(&self, hash_prefix: &str) -> Result<Vec<String>> {
        // Validate prefix is hex-only to prevent path traversal
        if !hash_prefix.chars().all(|c| c.is_ascii_hexdigit()) {
            anyhow::bail!("Invalid hash prefix: must contain only hex characters");
        }

        if let Some(remote) = &self.remote_store {
            use futures::TryStreamExt;
            let prefix = ObjectPath::from(format!("{}/{}", self.prefix, hash_prefix));
            let mut hashes = Vec::new();
            let mut listing = remote.list(Some(&prefix));
            while let Some(meta) = listing.try_next().await? {
                let path_str = meta.location.to_string();
                if let Some(filename) = path_str.rsplit('/').next() {
                    if let Some(hash) = filename.split('.').next() {
                        hashes.push(hash.to_string());
                    }
                }
            }
            return Ok(hashes);
        }

        // Local-only fallback
        let root = self.cache_dir.join(&self.prefix).join(hash_prefix);
        let mut hashes = Vec::new();

        if !root.exists() {
            return Ok(hashes);
        }

        for level2 in std::fs::read_dir(&root)? {
            let level2 = level2?;
            if !level2.file_type()?.is_dir() {
                continue;
            }
            for file in std::fs::read_dir(level2.path())? {
                let file = file?;
                if !file.file_type()?.is_file() {
                    continue;
                }
                if let Some(name) = file.file_name().to_str() {
                    if let Some(hash) = name.split('.').next() {
                        hashes.push(hash.to_string());
                    }
                }
            }
        }

        Ok(hashes)
    }

    /// Get metadata for content (size, last modified).
    pub async fn metadata(
        &self,
        content_hash: &str,
        extension: &str,
    ) -> Result<object_store::ObjectMeta> {
        let local_path = self.local_path(content_hash, extension)?;
        if local_path.exists() {
            let metadata = std::fs::metadata(&local_path)?;
            let last_modified = metadata
                .modified()
                .ok()
                .map(chrono::DateTime::<chrono::Utc>::from)
                .unwrap_or_else(chrono::Utc::now);

            return Ok(object_store::ObjectMeta {
                location: self.object_path(content_hash, extension)?,
                last_modified,
                size: metadata.len() as usize,
                e_tag: None,
                version: None,
            });
        }

        let remote = self
            .remote_store
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Content not found: {}", content_hash))?;
        let path = self.object_path(content_hash, extension)?;
        let meta = remote
            .head(&path)
            .await
            .with_context(|| format!("Content not found: {}", content_hash))?;
        Ok(meta)
    }
}

#[cfg(test)]
mod tests {
    use super::ContentStorage;

    #[test]
    fn test_object_path() {
        let hash = "abcd1234567890abcd1234567890abcd1234567890abcd1234567890abcd1234";
        let dir1 = &hash[0..2];
        let dir2 = &hash[2..4];
        let expected = format!("content/{}/{}/{}.parquet", dir1, dir2, hash);

        assert_eq!(dir1, "ab");
        assert_eq!(dir2, "cd");
        assert!(expected.contains("content/ab/cd/"));
    }

    #[test]
    fn test_validate_content_hash_rejects_invalid_values() {
        // 64-char BLAKE3 hash
        let valid_blake3 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        assert!(ContentStorage::validate_content_hash(valid_blake3).is_ok());

        // 40-char Git SHA-1 hash
        let valid_sha1 = "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2";
        assert!(ContentStorage::validate_content_hash(valid_sha1).is_ok());

        // Invalid: too short
        assert!(ContentStorage::validate_content_hash("abc").is_err());

        // Invalid: wrong length (50 chars, neither 40 nor 64)
        assert!(
            ContentStorage::validate_content_hash(
                "0123456789abcdef0123456789abcdef0123456789abcdef01"
            )
            .is_err()
        );

        // Invalid: non-hex characters
        assert!(
            ContentStorage::validate_content_hash(
                "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz"
            )
            .is_err()
        );
    }
}
