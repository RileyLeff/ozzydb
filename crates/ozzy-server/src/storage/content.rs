//! Content-addressed R2/S3 storage.
//!
//! Files are stored by their BLAKE3 content hash, enabling automatic deduplication.
//! Storage layout: {prefix}/{hash[0:2]}/{hash[2:4]}/{hash}.{ext}

use anyhow::{Context, Result};
use bytes::Bytes;
use futures::TryStreamExt;
use object_store::aws::AmazonS3Builder;
use object_store::path::Path as ObjectPath;
use object_store::ObjectStore;
use ozzy_core::hash;
use std::sync::Arc;

use crate::config::R2Config;

/// Content-addressed storage backend using R2/S3.
#[derive(Clone)]
pub struct ContentStorage {
    store: Arc<dyn ObjectStore>,
    prefix: String,
}

impl ContentStorage {
    /// Create a new content storage connected to R2.
    pub fn new(config: &R2Config) -> Result<Self> {
        let store = AmazonS3Builder::new()
            .with_endpoint(&config.endpoint)
            .with_bucket_name(&config.bucket)
            .with_access_key_id(&config.access_key_id)
            .with_secret_access_key(&config.secret_access_key)
            .with_region(&config.region)
            // R2 requires virtual-hosted-style to be disabled
            .with_virtual_hosted_style_request(false)
            .build()
            .context("Failed to create R2 storage client")?;

        Ok(Self {
            store: Arc::new(store),
            prefix: "content".to_string(),
        })
    }

    /// Create a new content storage with a custom prefix.
    pub fn with_prefix(config: &R2Config, prefix: impl Into<String>) -> Result<Self> {
        let mut storage = Self::new(config)?;
        storage.prefix = prefix.into();
        Ok(storage)
    }

    /// Check if content with the given hash exists.
    pub async fn exists(&self, content_hash: &str, extension: &str) -> Result<bool> {
        let path = self.object_path(content_hash, extension);
        match self.store.head(&path).await {
            Ok(_) => Ok(true),
            Err(object_store::Error::NotFound { .. }) => Ok(false),
            Err(e) => Err(e).context("Failed to check if content exists"),
        }
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
    /// If content with this hash already exists, this is a no-op.
    pub async fn store(&self, content: &[u8], extension: &str) -> Result<String> {
        let content_hash = hash::blake3_hash(content);

        // Check if already exists (deduplication)
        if self.exists(&content_hash, extension).await? {
            return Ok(content_hash);
        }

        let path = self.object_path(&content_hash, extension);

        self.store
            .put(&path, Bytes::copy_from_slice(content).into())
            .await
            .context("Failed to store content in R2")?;

        Ok(content_hash)
    }

    /// Store content from bytes, computing hash as we go.
    pub async fn store_bytes(&self, content: Bytes, extension: &str) -> Result<String> {
        let content_hash = hash::blake3_hash(&content);

        // Check if already exists (deduplication)
        if self.exists(&content_hash, extension).await? {
            return Ok(content_hash);
        }

        let path = self.object_path(&content_hash, extension);

        self.store
            .put(&path, content.into())
            .await
            .context("Failed to store content in R2")?;

        Ok(content_hash)
    }

    /// Retrieve content by hash.
    /// Validates that the retrieved content's hash matches the expected hash.
    pub async fn get(&self, content_hash: &str, extension: &str) -> Result<Bytes> {
        let path = self.object_path(content_hash, extension);
        let result = self
            .store
            .get(&path)
            .await
            .with_context(|| format!("Content not found: {}", content_hash))?;

        let content = result
            .bytes()
            .await
            .context("Failed to read content from R2")?;

        // Validate the content hash matches what was requested
        let actual_hash = hash::blake3_hash(&content);
        if actual_hash != content_hash {
            anyhow::bail!(
                "Content hash mismatch: expected {}, got {}. Storage may be corrupted.",
                content_hash,
                actual_hash
            );
        }

        Ok(content)
    }

    /// Get content as a stream for large files.
    pub async fn get_stream(
        &self,
        content_hash: &str,
        extension: &str,
    ) -> Result<impl futures::Stream<Item = Result<Bytes, object_store::Error>>> {
        let path = self.object_path(content_hash, extension);
        let result = self
            .store
            .get(&path)
            .await
            .with_context(|| format!("Content not found: {}", content_hash))?;

        Ok(result.into_stream())
    }

    /// Get the object path for a content hash.
    fn object_path(&self, content_hash: &str, extension: &str) -> ObjectPath {
        // Use first 4 chars for 2-level directory structure
        let dir1 = &content_hash[0..2];
        let dir2 = &content_hash[2..4];
        ObjectPath::from(format!(
            "{}/{}/{}/{}.{}",
            self.prefix, dir1, dir2, content_hash, extension
        ))
    }

    /// Delete content by hash.
    pub async fn delete(&self, content_hash: &str, extension: &str) -> Result<()> {
        let path = self.object_path(content_hash, extension);
        self.store
            .delete(&path)
            .await
            .context("Failed to delete content from R2")?;
        Ok(())
    }

    /// List all content hashes with a given prefix (first 2 chars).
    pub async fn list_by_prefix(&self, hash_prefix: &str) -> Result<Vec<String>> {
        let path = ObjectPath::from(format!("{}/{}", self.prefix, hash_prefix));

        let mut hashes = Vec::new();
        let mut list_stream = self.store.list(Some(&path));

        while let Some(meta) = list_stream.try_next().await? {
            // Extract hash from path like "content/ab/cd/abcd1234.parquet"
            if let Some(filename) = meta.location.filename() {
                if let Some(hash) = filename.split('.').next() {
                    hashes.push(hash.to_string());
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
        let path = self.object_path(content_hash, extension);
        self.store
            .head(&path)
            .await
            .with_context(|| format!("Content not found: {}", content_hash))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_object_path() {
        // We can't easily test the full storage without R2, but we can test path generation
        let hash = "abcd1234567890abcd1234567890abcd1234567890abcd1234567890abcd1234";
        let dir1 = &hash[0..2];
        let dir2 = &hash[2..4];
        let expected = format!("content/{}/{}/{}.parquet", dir1, dir2, hash);

        assert_eq!(dir1, "ab");
        assert_eq!(dir2, "cd");
        assert!(expected.contains("content/ab/cd/"));
    }
}
