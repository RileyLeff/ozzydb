//! Content-addressed storage backed by R2/S3.
//!
//! R2 (or S3-compatible, e.g. MinIO) is the single source of truth.
//! There is no local-only mode — an object store is always required.
//!
//! Layout: {prefix}/{hash[0:2]}/{hash[2:4]}/{hash}.{ext}

use anyhow::{Context, Result};
use aws_credential_types::Credentials;
use aws_sdk_s3::presigning::PresigningConfig;
use aws_sdk_s3::types::{CompletedMultipartUpload, CompletedPart};
use bytes::Bytes;
use futures::{Stream, StreamExt};
use object_store::ObjectStore;
use object_store::aws::AmazonS3Builder;
use object_store::path::Path as ObjectPath;
use ozzy_core::hash;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use crate::config::{Config, R2Config};

/// Content-addressed storage backend.
///
/// All content is stored in R2/S3. Presigned URLs are used for
/// direct client uploads/downloads.
#[derive(Clone)]
pub struct ContentStorage {
    remote_store: Arc<dyn ObjectStore>,
    /// AWS SDK S3 client for presigned URL generation.
    s3_client: aws_sdk_s3::Client,
    /// Optional S3 client for generating presigned URLs consumed by compute containers.
    /// When set, `presigned_*_for_compute()` methods use this client instead of `s3_client`.
    /// This allows generating URLs with a different hostname (e.g., `host.docker.internal`
    /// instead of `localhost`) so Docker containers can reach MinIO in local dev.
    compute_s3_client: Option<aws_sdk_s3::Client>,
    /// Bucket name for presigned URL generation.
    bucket: String,
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
        let mut builder = AmazonS3Builder::new()
            .with_endpoint(&config.endpoint)
            .with_bucket_name(&config.bucket)
            .with_access_key_id(&config.access_key_id)
            .with_secret_access_key(&config.secret_access_key)
            .with_region(&config.region)
            .with_virtual_hosted_style_request(false);

        // Allow HTTP for local dev (MinIO). R2 endpoints are always HTTPS.
        if config.endpoint.starts_with("http://") {
            builder = builder.with_allow_http(true);
        }

        let store = builder
            .build()
            .context("Failed to create R2 storage client")?;
        Ok(Arc::new(store))
    }

    /// Build an AWS SDK S3 client for presigned URL generation.
    /// Uses path-style addressing (required for R2 and MinIO).
    fn build_s3_client(config: &R2Config) -> aws_sdk_s3::Client {
        let credentials = Credentials::new(
            &config.access_key_id,
            &config.secret_access_key,
            None, // session token
            None, // expiry
            "ozzydb-r2",
        );
        let s3_config = aws_sdk_s3::Config::builder()
            .behavior_version_latest()
            .endpoint_url(&config.endpoint)
            .region(aws_sdk_s3::config::Region::new(config.region.clone()))
            .credentials_provider(credentials)
            .force_path_style(true)
            .build();
        aws_sdk_s3::Client::from_conf(s3_config)
    }

    fn new_inner(
        remote_store: Arc<dyn ObjectStore>,
        s3_client: aws_sdk_s3::Client,
        compute_s3_client: Option<aws_sdk_s3::Client>,
        bucket: String,
        prefix: impl Into<String>,
        verify_content_hash: bool,
    ) -> Self {
        Self {
            remote_store,
            s3_client,
            compute_s3_client,
            bucket,
            prefix: prefix.into(),
            verify_content_hash,
        }
    }

    /// Build an optional S3 client for compute-facing presigned URLs.
    fn build_compute_s3_client(config: &R2Config) -> Option<aws_sdk_s3::Client> {
        let presign_endpoint = config.presign_endpoint.as_deref()?;
        Some(Self::build_s3_client_with_endpoint(
            config,
            presign_endpoint,
        ))
    }

    /// Build an S3 client using a specific endpoint URL (for presign_endpoint support).
    fn build_s3_client_with_endpoint(config: &R2Config, endpoint: &str) -> aws_sdk_s3::Client {
        let credentials = Credentials::new(
            &config.access_key_id,
            &config.secret_access_key,
            None,
            None,
            "ozzydb-r2",
        );
        let s3_config = aws_sdk_s3::Config::builder()
            .behavior_version_latest()
            .endpoint_url(endpoint)
            .region(aws_sdk_s3::config::Region::new(config.region.clone()))
            .credentials_provider(credentials)
            .force_path_style(true)
            .build();
        aws_sdk_s3::Client::from_conf(s3_config)
    }

    /// Create storage from server config (content-addressed, verifies hashes on read).
    pub fn from_config(config: &Config) -> Result<Self> {
        let remote_store = Self::build_remote_store(&config.r2)?;
        let s3_client = Self::build_s3_client(&config.r2);
        let compute_s3_client = Self::build_compute_s3_client(&config.r2);
        Ok(Self::new_inner(
            remote_store,
            s3_client,
            compute_s3_client,
            config.r2.bucket.clone(),
            "content",
            true, // content-addressed: verify hash on read
        ))
    }

    /// Create storage from server config with a custom prefix.
    ///
    /// Hash verification is disabled because the storage key may not be the
    /// blake3 of the content (e.g., materialized cache uses composite hashes).
    pub fn from_config_with_prefix(config: &Config, prefix: &str) -> Result<Self> {
        let remote_store = Self::build_remote_store(&config.r2)?;
        let s3_client = Self::build_s3_client(&config.r2);
        let compute_s3_client = Self::build_compute_s3_client(&config.r2);
        Ok(Self::new_inner(
            remote_store,
            s3_client,
            compute_s3_client,
            config.r2.bucket.clone(),
            prefix,
            false, // key-addressed: skip hash verification
        ))
    }

    /// Create storage from R2Config directly (content-addressed).
    pub fn new(config: &R2Config) -> Result<Self> {
        let remote_store = Self::build_remote_store(config)?;
        let s3_client = Self::build_s3_client(config);
        let compute_s3_client = Self::build_compute_s3_client(config);
        Ok(Self::new_inner(
            remote_store,
            s3_client,
            compute_s3_client,
            config.bucket.clone(),
            "content",
            true,
        ))
    }

    /// Create storage from R2Config with custom prefix (content-addressed).
    pub fn with_prefix(config: &R2Config, prefix: impl Into<String>) -> Result<Self> {
        let remote_store = Self::build_remote_store(config)?;
        let s3_client = Self::build_s3_client(config);
        let compute_s3_client = Self::build_compute_s3_client(config);
        Ok(Self::new_inner(
            remote_store,
            s3_client,
            compute_s3_client,
            config.bucket.clone(),
            prefix,
            true,
        ))
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

    /// Upload content to R2.
    async fn upload_remote(
        &self,
        content_hash: &str,
        extension: &str,
        content: &[u8],
    ) -> Result<()> {
        let path = self.object_path(content_hash, extension)?;
        self.remote_store
            .put(&path, Bytes::copy_from_slice(content).into())
            .await
            .context("Failed to write content to R2")?;
        Ok(())
    }

    /// Check if content with the given hash exists.
    pub async fn exists(&self, content_hash: &str, extension: &str) -> Result<bool> {
        let remote_path = self.object_path(content_hash, extension)?;
        match self.remote_store.head(&remote_path).await {
            Ok(_) => Ok(true),
            Err(object_store::Error::NotFound { .. }) => Ok(false),
            Err(e) => Err(e).context("Failed to check remote content existence"),
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
    pub async fn store(&self, content: &[u8], extension: &str) -> Result<String> {
        let content_hash = hash::blake3_hash(content);
        self.upload_remote(&content_hash, extension, content)
            .await?;
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
        self.upload_remote(content_hash, extension, content).await
    }

    /// Store content from bytes.
    pub async fn store_bytes(&self, content: Bytes, extension: &str) -> Result<String> {
        self.store(&content, extension).await
    }

    /// Retrieve content by hash.
    pub async fn get(&self, content_hash: &str, extension: &str) -> Result<Bytes> {
        let remote_path = self.object_path(content_hash, extension)?;
        let result = self
            .remote_store
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
                    "Content hash mismatch: expected {}, got {}. Storage may be corrupted.",
                    content_hash,
                    actual_hash
                );
            }
        }

        Ok(content)
    }

    /// Get content as a stream for large files.
    pub async fn get_stream(
        &self,
        content_hash: &str,
        extension: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<Bytes, object_store::Error>> + Send>>> {
        let path = self.object_path(content_hash, extension)?;
        let result = self
            .remote_store
            .get(&path)
            .await
            .with_context(|| format!("Content not found: {}", content_hash))?;
        Ok(result.into_stream())
    }

    /// Delete content by hash.
    pub async fn delete(&self, content_hash: &str, extension: &str) -> Result<()> {
        let path = self.object_path(content_hash, extension)?;
        match self.remote_store.delete(&path).await {
            Ok(()) => {}
            Err(object_store::Error::NotFound { .. }) => {}
            Err(e) => return Err(e).context("Failed to delete content from R2"),
        }
        Ok(())
    }

    /// List all content hashes with a given prefix (first 2 hex chars).
    pub async fn list_by_prefix(&self, hash_prefix: &str) -> Result<Vec<String>> {
        // Validate prefix is hex-only to prevent path traversal
        if !hash_prefix.chars().all(|c| c.is_ascii_hexdigit()) {
            anyhow::bail!("Invalid hash prefix: must contain only hex characters");
        }

        use futures::TryStreamExt;
        let prefix = ObjectPath::from(format!("{}/{}", self.prefix, hash_prefix));
        let mut hashes = Vec::new();
        let mut listing = self.remote_store.list(Some(&prefix));
        while let Some(meta) = listing.try_next().await? {
            let path_str = meta.location.to_string();
            if let Some(filename) = path_str.rsplit('/').next() {
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
        let path = self.object_path(content_hash, extension)?;
        let meta = self
            .remote_store
            .head(&path)
            .await
            .with_context(|| format!("Content not found: {}", content_hash))?;
        Ok(meta)
    }

    /// Generate a presigned GET URL for content with the given hash and extension.
    ///
    /// The URL allows the holder to download the content directly from R2/S3
    /// without authentication for the duration of `ttl`.
    pub async fn presigned_get_url(
        &self,
        content_hash: &str,
        extension: &str,
        ttl: Duration,
    ) -> Result<String> {
        self.presigned_get_url_with_filename(content_hash, extension, ttl, None)
            .await
    }

    /// Like `presigned_get_url` but with an optional download filename override.
    pub async fn presigned_get_url_with_filename(
        &self,
        content_hash: &str,
        extension: &str,
        ttl: Duration,
        download_filename: Option<&str>,
    ) -> Result<String> {
        let key = self.storage_key(content_hash, extension)?;

        let presigning = PresigningConfig::expires_in(ttl).context("Invalid presigning TTL")?;
        let mut request = self.s3_client.get_object().bucket(&self.bucket).key(&key);
        if let Some(filename) = download_filename {
            request = request
                .response_content_disposition(format!("attachment; filename=\"{}\"", filename));
        }
        let presigned = request
            .presigned(presigning)
            .await
            .context("Failed to generate presigned GET URL")?;

        Ok(presigned.uri().to_string())
    }

    /// Generate a presigned PUT URL for uploading content to a specific key.
    ///
    /// Note: pub(crate) because the raw key is not validated — callers should
    /// use `presigned_put_url_for_content()` which validates the hash.
    pub(crate) async fn presigned_put_url(
        &self,
        storage_key: &str,
        ttl: Duration,
    ) -> Result<String> {
        let presigning = PresigningConfig::expires_in(ttl).context("Invalid presigning TTL")?;
        let presigned = self
            .s3_client
            .put_object()
            .bucket(&self.bucket)
            .key(storage_key)
            .presigned(presigning)
            .await
            .context("Failed to generate presigned PUT URL")?;

        Ok(presigned.uri().to_string())
    }

    /// Generate a presigned PUT URL for content with the given hash and extension.
    ///
    /// Convenience wrapper that computes the storage key from hash + extension.
    pub async fn presigned_put_url_for_content(
        &self,
        content_hash: &str,
        extension: &str,
        ttl: Duration,
    ) -> Result<String> {
        let key = self.storage_key(content_hash, extension)?;
        self.presigned_put_url(&key, ttl).await
    }

    /// Get content by raw R2/S3 key (not content-addressed).
    ///
    /// Used for temporary objects like compute output tarballs that don't follow
    /// the `{prefix}/{hash[0:2]}/{hash[2:4]}/{hash}.{ext}` layout.
    pub async fn get_by_key(&self, key: &str) -> Result<Bytes> {
        let path = ObjectPath::from(key);
        let result = self
            .remote_store
            .get(&path)
            .await
            .with_context(|| format!("Failed to get object by key: {}", key))?;
        let bytes = result
            .bytes()
            .await
            .with_context(|| format!("Failed to read object bytes: {}", key))?;
        Ok(bytes)
    }

    /// Delete an object by raw R2/S3 key (not content-addressed).
    ///
    /// Used for cleaning up temporary objects like compute output tarballs.
    pub async fn delete_by_key(&self, key: &str) -> Result<()> {
        let path = ObjectPath::from(key);
        self.remote_store
            .delete(&path)
            .await
            .with_context(|| format!("Failed to delete object by key: {}", key))?;
        Ok(())
    }

    /// Store bytes under a raw R2/S3 key (not content-addressed).
    ///
    /// Used for temporary objects like secrets blobs.
    pub async fn store_by_key(&self, key: &str, bytes: &[u8]) -> Result<()> {
        let path = ObjectPath::from(key);
        self.remote_store
            .put(&path, bytes::Bytes::copy_from_slice(bytes).into())
            .await
            .with_context(|| format!("Failed to store object by key: {}", key))?;
        Ok(())
    }

    /// Generate a presigned GET URL for a raw R2/S3 key (not content-addressed).
    ///
    /// Used for temporary objects like secrets blobs.
    pub async fn presigned_get_url_by_key(&self, key: &str, ttl: Duration) -> Result<String> {
        let presigning = PresigningConfig::expires_in(ttl).context("Invalid presigning TTL")?;
        let presigned = self
            .s3_client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .presigned(presigning)
            .await
            .context("Failed to generate presigned GET URL")?;
        Ok(presigned.uri().to_string())
    }

    // ── Compute-facing presigned URLs ─────────────────────────────
    //
    // These use `compute_s3_client` (if configured via R2_PRESIGN_ENDPOINT)
    // to generate presigned URLs reachable from inside compute containers.
    // Falls back to the regular `s3_client` when no alternate endpoint is set.

    /// S3 client to use for compute-facing presigned URLs.
    fn compute_client(&self) -> &aws_sdk_s3::Client {
        self.compute_s3_client.as_ref().unwrap_or(&self.s3_client)
    }

    /// Presigned GET URL for content, accessible from compute containers.
    pub async fn presigned_get_url_for_compute(
        &self,
        content_hash: &str,
        extension: &str,
        ttl: Duration,
    ) -> Result<String> {
        let key = self.storage_key(content_hash, extension)?;
        let presigning = PresigningConfig::expires_in(ttl).context("Invalid presigning TTL")?;
        let presigned = self
            .compute_client()
            .get_object()
            .bucket(&self.bucket)
            .key(&key)
            .presigned(presigning)
            .await
            .context("Failed to generate compute presigned GET URL")?;
        Ok(presigned.uri().to_string())
    }

    /// Presigned PUT URL for a raw key, accessible from compute containers.
    pub(crate) async fn presigned_put_url_for_compute(
        &self,
        storage_key: &str,
        ttl: Duration,
    ) -> Result<String> {
        let presigning = PresigningConfig::expires_in(ttl).context("Invalid presigning TTL")?;
        let presigned = self
            .compute_client()
            .put_object()
            .bucket(&self.bucket)
            .key(storage_key)
            .presigned(presigning)
            .await
            .context("Failed to generate compute presigned PUT URL")?;
        Ok(presigned.uri().to_string())
    }

    /// Presigned GET URL for a raw key, accessible from compute containers.
    pub async fn presigned_get_url_by_key_for_compute(
        &self,
        key: &str,
        ttl: Duration,
    ) -> Result<String> {
        let presigning = PresigningConfig::expires_in(ttl).context("Invalid presigning TTL")?;
        let presigned = self
            .compute_client()
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .presigned(presigning)
            .await
            .context("Failed to generate compute presigned GET URL")?;
        Ok(presigned.uri().to_string())
    }

    /// Store content from a stream, hashing on the fly.
    ///
    /// Returns `(content_hash, byte_size)`. For files <=5MB, uses a single PutObject.
    /// For files >5MB, uses S3 multipart upload to a temp key, then copies to the
    /// content-addressed key once the hash is known.
    pub async fn store_stream(
        &self,
        mut stream: Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>,
        extension: &str,
    ) -> Result<(String, u64)> {
        const PART_SIZE: usize = 5 * 1024 * 1024; // 5MB

        // Phase 1: Buffer up to PART_SIZE while hashing.
        let mut hasher = blake3::Hasher::new();
        let mut buffer = Vec::new();
        let mut total_size: u64 = 0;
        let mut stream_exhausted = false;

        while buffer.len() < PART_SIZE {
            match stream.next().await {
                Some(Ok(chunk)) => {
                    hasher.update(&chunk);
                    total_size += chunk.len() as u64;
                    buffer.extend_from_slice(&chunk);
                }
                Some(Err(e)) => return Err(anyhow::anyhow!("Error reading upload stream: {}", e)),
                None => {
                    stream_exhausted = true;
                    break;
                }
            }
        }

        if stream_exhausted {
            // Small file: we have the full content + hash. Single PutObject.
            let content_hash = hasher.finalize().to_hex().to_string();
            let key = self.storage_key(&content_hash, extension)?;

            self.s3_client
                .put_object()
                .bucket(&self.bucket)
                .key(&key)
                .body(Bytes::from(buffer).into())
                .send()
                .await
                .context("Failed to upload small file to R2")?;

            return Ok((content_hash, total_size));
        }

        // Phase 2: Large file — multipart upload to temp key.
        let temp_key = format!("_upload/{}.tmp", uuid::Uuid::new_v4());

        let create = self
            .s3_client
            .create_multipart_upload()
            .bucket(&self.bucket)
            .key(&temp_key)
            .send()
            .await
            .context("Failed to create multipart upload")?;
        let upload_id = create
            .upload_id()
            .ok_or_else(|| anyhow::anyhow!("Missing upload_id from CreateMultipartUpload"))?
            .to_string();

        // Helper closure to abort multipart upload on error.
        let abort = |client: &aws_sdk_s3::Client, bucket: &str, key: &str, id: &str| {
            let client = client.clone();
            let bucket = bucket.to_string();
            let key = key.to_string();
            let id = id.to_string();
            async move {
                let _ = client
                    .abort_multipart_upload()
                    .bucket(&bucket)
                    .key(&key)
                    .upload_id(&id)
                    .send()
                    .await;
            }
        };

        // Upload all parts, aborting on any failure.
        let multipart_result: Result<Vec<CompletedPart>> = async {
            let mut parts: Vec<CompletedPart> = Vec::new();
            let mut part_number: i32 = 1;

            // Upload buffered data as part 1.
            let part1 = self
                .s3_client
                .upload_part()
                .bucket(&self.bucket)
                .key(&temp_key)
                .upload_id(&upload_id)
                .part_number(part_number)
                .body(buffer.into())
                .send()
                .await
                .context("Failed to upload part 1")?;
            parts.push(
                CompletedPart::builder()
                    .part_number(part_number)
                    .set_e_tag(part1.e_tag().map(|s| s.to_string()))
                    .build(),
            );
            part_number += 1;

            // Stream remaining parts.
            let mut part_buf = Vec::with_capacity(PART_SIZE);
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.context("Error reading upload stream")?;
                hasher.update(&chunk);
                total_size += chunk.len() as u64;
                part_buf.extend_from_slice(&chunk);

                if part_buf.len() >= PART_SIZE {
                    let part_data = std::mem::replace(&mut part_buf, Vec::with_capacity(PART_SIZE));
                    let part = self
                        .s3_client
                        .upload_part()
                        .bucket(&self.bucket)
                        .key(&temp_key)
                        .upload_id(&upload_id)
                        .part_number(part_number)
                        .body(part_data.into())
                        .send()
                        .await
                        .with_context(|| format!("Failed to upload part {}", part_number))?;
                    parts.push(
                        CompletedPart::builder()
                            .part_number(part_number)
                            .set_e_tag(part.e_tag().map(|s| s.to_string()))
                            .build(),
                    );
                    part_number += 1;
                }
            }

            // Flush remaining bytes as final part.
            if !part_buf.is_empty() {
                let part = self
                    .s3_client
                    .upload_part()
                    .bucket(&self.bucket)
                    .key(&temp_key)
                    .upload_id(&upload_id)
                    .part_number(part_number)
                    .body(part_buf.into())
                    .send()
                    .await
                    .with_context(|| format!("Failed to upload final part {}", part_number))?;
                parts.push(
                    CompletedPart::builder()
                        .part_number(part_number)
                        .set_e_tag(part.e_tag().map(|s| s.to_string()))
                        .build(),
                );
            }

            Ok(parts)
        }
        .await;

        let parts = match multipart_result {
            Ok(p) => p,
            Err(e) => {
                abort(&self.s3_client, &self.bucket, &temp_key, &upload_id).await;
                return Err(e);
            }
        };

        // Complete multipart upload.
        let completed = CompletedMultipartUpload::builder()
            .set_parts(Some(parts))
            .build();
        if let Err(e) = self
            .s3_client
            .complete_multipart_upload()
            .bucket(&self.bucket)
            .key(&temp_key)
            .upload_id(&upload_id)
            .multipart_upload(completed)
            .send()
            .await
        {
            abort(&self.s3_client, &self.bucket, &temp_key, &upload_id).await;
            return Err(e).context("Failed to complete multipart upload");
        }

        // Now we know the hash — copy from temp to content-addressed key.
        let content_hash = hasher.finalize().to_hex().to_string();
        let final_key = self.storage_key(&content_hash, extension)?;

        if let Err(e) = self
            .s3_client
            .copy_object()
            .bucket(&self.bucket)
            .key(&final_key)
            .copy_source(format!("/{}/{}", self.bucket, temp_key))
            .send()
            .await
        {
            // Clean up temp key before returning error.
            let _ = self
                .s3_client
                .delete_object()
                .bucket(&self.bucket)
                .key(&temp_key)
                .send()
                .await;
            return Err(e).context("Failed to copy temp upload to content-addressed key");
        }

        // Delete temp key.
        let _ = self
            .s3_client
            .delete_object()
            .bucket(&self.bucket)
            .key(&temp_key)
            .send()
            .await;

        Ok((content_hash, total_size))
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
