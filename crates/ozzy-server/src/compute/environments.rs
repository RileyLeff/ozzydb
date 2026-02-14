//! Environment image management for compute providers.
//!
//! Tracks which environment images are available on which providers
//! (Docker, Fly, etc.) via the `environment_provider_images` DB table.
//! When a compute job needs an environment on a specific provider,
//! this module checks availability and returns the provider-specific image ref.

use anyhow::Result;

use crate::db::Database;

/// Provider identifiers.
pub const PROVIDER_DOCKER: &str = "docker";
pub const PROVIDER_FLY: &str = "fly";

/// Get the image reference for an environment on the active provider.
///
/// For Docker: the image ref is a local Docker image tag (e.g., "ozzydb-env:abc123").
/// For Fly: the image ref is a Fly registry URL (e.g., "registry.fly.io/ozzydb-compute:abc123").
///
/// Returns None if the environment hasn't been pushed to the requested provider.
pub async fn get_image_ref(
    db: &Database,
    env_hash: &str,
    provider: &str,
) -> Result<Option<String>> {
    let record = db.get_provider_image(env_hash, provider).await?;
    Ok(record.map(|r| r.image_ref))
}

/// Ensure an environment image is available on the target provider.
///
/// If the image is already tracked in the DB for this provider, returns the
/// existing image ref. Otherwise, returns None (the caller must push the image
/// and then call `register_image`).
///
/// Future enhancement: automatically push to provider if not available.
pub async fn ensure_available(
    db: &Database,
    env_hash: &str,
    provider: &str,
) -> Result<Option<String>> {
    get_image_ref(db, env_hash, provider).await
}

/// Register an environment image as available on a provider.
///
/// Called after successfully pushing an image to a provider's registry.
pub async fn register_image(
    db: &Database,
    env_hash: &str,
    provider: &str,
    image_ref: &str,
) -> Result<()> {
    db.upsert_provider_image(env_hash, provider, image_ref)
        .await?;
    Ok(())
}

/// Format the expected Fly registry image reference for an environment hash.
///
/// Convention: `registry.fly.io/{app_name}:{env_hash_prefix}`
pub fn fly_image_ref(app_name: &str, env_hash: &str) -> String {
    let prefix = env_hash.get(..12).unwrap_or(env_hash);
    format!("registry.fly.io/{}:{}", app_name, prefix)
}

/// Format the expected Docker image reference for an environment hash.
///
/// Convention: `ozzydb-env:{env_hash_prefix}`
pub fn docker_image_ref(env_hash: &str) -> String {
    let prefix = env_hash.get(..12).unwrap_or(env_hash);
    format!("ozzydb-env:{}", prefix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fly_image_ref() {
        let hash = "abcdef123456789012345678901234567890123456789012345678901234";
        assert_eq!(
            fly_image_ref("ozzydb-compute", hash),
            "registry.fly.io/ozzydb-compute:abcdef123456"
        );
    }

    #[test]
    fn test_docker_image_ref() {
        let hash = "abcdef123456789012345678901234567890123456789012345678901234";
        assert_eq!(docker_image_ref(hash), "ozzydb-env:abcdef123456");
    }

    #[test]
    fn test_fly_image_ref_short_hash() {
        let hash = "abc";
        assert_eq!(
            fly_image_ref("ozzydb-compute", hash),
            "registry.fly.io/ozzydb-compute:abc"
        );
    }
}
