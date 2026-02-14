//! Secrets delivery for compute — presigned-URL-based injection.
//!
//! For Fly machines, secrets can't be passed as raw env vars (they'd be visible
//! in the Fly API). Instead:
//! 1. Build a JSON blob of all secret key=value pairs
//! 2. Upload to R2 under a temp key
//! 3. Generate a short-lived presigned GET URL
//! 4. Pass OZZY_SECRETS_URL as env var
//! 5. Init script downloads + exports the secrets
//! 6. After job completes, delete the R2 blob

use anyhow::Result;
use uuid::Uuid;

use crate::storage::ContentStorage;

/// Result of preparing secrets for a compute job.
pub struct PreparedSecrets {
    /// Presigned URL to download the secrets JSON blob.
    pub url: String,
    /// R2 key for cleanup after job completes.
    pub r2_key: String,
}

/// Prepare secrets for delivery to a compute machine.
///
/// Uploads a JSON blob of `{ "SECRET_NAME": "value", ... }` to R2 under a
/// temporary key, and returns a presigned GET URL with a TTL tied to the
/// compute timeout (timeout + 5 min buffer).
pub async fn prepare_secrets(
    storage: &ContentStorage,
    job_id: Uuid,
    secrets: &std::collections::HashMap<String, String>,
    timeout_secs: u64,
) -> Result<PreparedSecrets> {
    let blob = serde_json::to_vec(secrets)?;
    let r2_key = format!("secrets/{}.json", job_id);

    // Upload the blob to R2
    storage.store_by_key(&r2_key, &blob).await?;

    // Generate a presigned GET URL (timeout + 5 min buffer)
    let ttl = std::time::Duration::from_secs(timeout_secs + 300);
    let url = storage.presigned_get_url_by_key(&r2_key, ttl).await?;

    Ok(PreparedSecrets { url, r2_key })
}

/// Clean up the secrets blob from R2 after a job completes.
pub async fn cleanup_secrets(storage: &ContentStorage, r2_key: &str) -> Result<()> {
    storage.delete_by_key(r2_key).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secrets_r2_key_format() {
        let job_id = Uuid::parse_str("12345678-1234-1234-1234-123456789abc").unwrap();
        let key = format!("secrets/{}.json", job_id);
        assert_eq!(key, "secrets/12345678-1234-1234-1234-123456789abc.json");
    }
}
