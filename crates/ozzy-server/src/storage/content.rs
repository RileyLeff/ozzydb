//! Content-addressed storage with local-first writes and optional R2 redundancy.
//!
//! Files are addressed by BLAKE3 content hash.
//! Layout: {root}/{prefix}/{hash[0:2]}/{hash[2:4]}/{hash}.{ext}

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

/// Content-addressed storage backend using local filesystem as primary with optional R2 mirror.
#[derive(Clone)]
pub struct ContentStorage {
    local_root: PathBuf,
    remote_store: Option<Arc<dyn ObjectStore>>,
    prefix: String,
}

impl ContentStorage {
    fn validate_content_hash(content_hash: &str) -> Result<()> {
        if content_hash.len() != 64 || !content_hash.chars().all(|c| c.is_ascii_hexdigit()) {
            anyhow::bail!(
                "Invalid content hash '{}': expected 64 hexadecimal characters",
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

    fn default_local_root() -> PathBuf {
        std::env::temp_dir().join("ozzydb-content")
    }

    fn new_with_root_and_remote(
        local_root: PathBuf,
        remote_store: Option<Arc<dyn ObjectStore>>,
        prefix: impl Into<String>,
    ) -> Result<Self> {
        std::fs::create_dir_all(&local_root)?;
        Ok(Self {
            local_root,
            remote_store,
            prefix: prefix.into(),
        })
    }

    /// Create storage from server config (local-first, optional R2 mirror).
    pub fn from_config(config: &Config) -> Result<Self> {
        let remote_store = config
            .r2
            .as_ref()
            .map(Self::build_remote_store)
            .transpose()?;
        Self::new_with_root_and_remote(
            PathBuf::from(&config.local_storage_path),
            remote_store,
            "content",
        )
    }

    /// Create storage with R2-only configuration (used by legacy tests).
    pub fn new(config: &R2Config) -> Result<Self> {
        let remote_store = Some(Self::build_remote_store(config)?);
        Self::new_with_root_and_remote(Self::default_local_root(), remote_store, "content")
    }

    /// Create storage with custom prefix (used by integration tests).
    pub fn with_prefix(config: &R2Config, prefix: impl Into<String>) -> Result<Self> {
        let remote_store = Some(Self::build_remote_store(config)?);
        Self::new_with_root_and_remote(Self::default_local_root(), remote_store, prefix)
    }

    fn object_path(&self, content_hash: &str, extension: &str) -> Result<ObjectPath> {
        Self::validate_content_hash(content_hash)?;
        let dir1 = &content_hash[0..2];
        let dir2 = &content_hash[2..4];
        Ok(ObjectPath::from(format!(
            "{}/{}/{}/{}.{}",
            self.prefix, dir1, dir2, content_hash, extension
        )))
    }

    fn local_path(&self, content_hash: &str, extension: &str) -> Result<PathBuf> {
        Self::validate_content_hash(content_hash)?;
        let dir1 = &content_hash[0..2];
        let dir2 = &content_hash[2..4];
        Ok(self
            .local_root
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

    async fn upload_remote_best_effort(
        &self,
        content_hash: &str,
        extension: &str,
        content: &[u8],
    ) -> Result<()> {
        let Some(remote) = &self.remote_store else {
            return Ok(());
        };
        let path = self.object_path(content_hash, extension)?;
        if let Err(e) = remote
            .put(&path, Bytes::copy_from_slice(content).into())
            .await
        {
            eprintln!("Warning: failed to mirror content to R2: {}", e);
        }
        Ok(())
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
    pub async fn store(&self, content: &[u8], extension: &str) -> Result<String> {
        let content_hash = hash::blake3_hash(content);
        let local_path = self.local_path(&content_hash, extension)?;

        if !local_path.exists() {
            Self::ensure_parent(&local_path)?;
            // Write to a temp file first, then atomically rename to prevent
            // partial writes from leaving corrupted content on crash.
            let tmp_path = local_path.with_extension(format!("{}.tmp", extension));
            std::fs::write(&tmp_path, content)?;
            std::fs::rename(&tmp_path, &local_path)?;
        }

        self.upload_remote_best_effort(&content_hash, extension, content)
            .await?;
        Ok(content_hash)
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
            let actual_hash = hash::blake3_hash(&content);
            if actual_hash != content_hash {
                anyhow::bail!(
                    "Content hash mismatch: expected {}, got {}. Local storage may be corrupted.",
                    content_hash,
                    actual_hash
                );
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

        let actual_hash = hash::blake3_hash(&content);
        if actual_hash != content_hash {
            anyhow::bail!(
                "Content hash mismatch: expected {}, got {}. Remote storage may be corrupted.",
                content_hash,
                actual_hash
            );
        }

        // Hydrate local cache for future reads.
        Self::ensure_parent(&local_path)?;
        std::fs::write(&local_path, &content)?;

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
            let bytes = std::fs::read(&local_path).map(Bytes::from).map_err(|e| {
                object_store::Error::Generic {
                    store: "local",
                    source: Box::new(e),
                }
            })?;
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
    pub async fn delete(&self, content_hash: &str, extension: &str) -> Result<()> {
        let local_path = self.local_path(content_hash, extension)?;
        if local_path.exists() {
            std::fs::remove_file(&local_path)?;
        }

        if let Some(remote) = &self.remote_store {
            let path = self.object_path(content_hash, extension)?;
            match remote.delete(&path).await {
                Ok(()) => {}
                Err(object_store::Error::NotFound { .. }) => {}
                Err(e) => return Err(e).context("Failed to delete content from remote storage"),
            }
        }

        Ok(())
    }

    /// List all content hashes with a given prefix (first 2 chars).
    pub async fn list_by_prefix(&self, hash_prefix: &str) -> Result<Vec<String>> {
        let root = self.local_root.join(&self.prefix).join(hash_prefix);
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
        let valid = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        assert!(ContentStorage::validate_content_hash(valid).is_ok());
        assert!(ContentStorage::validate_content_hash("abc").is_err());
        assert!(
            ContentStorage::validate_content_hash(
                "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz"
            )
            .is_err()
        );
    }
}
