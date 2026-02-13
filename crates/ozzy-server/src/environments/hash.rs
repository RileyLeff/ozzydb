//! Environment hash computation.
//!
//! Environment hashes are content-addressed identifiers for built images:
//! - Tier 1 (BaseLockfile): `blake3(base_image + lockfile_content)`
//! - Tier 2 (Dockerfile): `blake3(dockerfile_content)`
//! - Tier 3 (Prebuilt): The image reference itself is used as-is (no hash computed)

use ozzy_core::hash::blake3_hash_components;
use ozzy_core::toml_spec::EnvironmentTier;

/// Compute the environment hash for a given tier.
///
/// All tiers produce a blake3 hash:
/// - BaseLockfile: `blake3(base_image + lockfile_content)`
/// - Dockerfile: `blake3(dockerfile_content)`
/// - Prebuilt: `blake3(image_reference)`
pub fn compute_env_hash(tier: &EnvironmentTier, content: &EnvironmentContent) -> String {
    match tier {
        EnvironmentTier::BaseLockfile { base, .. } => {
            let lockfile_content = content.lockfile_content.as_deref().unwrap_or("");
            blake3_hash_components(&[base, lockfile_content])
        }
        EnvironmentTier::Dockerfile { .. } => {
            let dockerfile_content = content.dockerfile_content.as_deref().unwrap_or("");
            ozzy_core::hash::blake3_hash(dockerfile_content.as_bytes())
        }
        EnvironmentTier::Prebuilt { image } => ozzy_core::hash::blake3_hash(image.as_bytes()),
    }
}

/// Content needed for environment hash computation.
///
/// Depending on the tier, different fields are populated:
/// - BaseLockfile: `lockfile_content` is required
/// - Dockerfile: `dockerfile_content` is required
/// - Prebuilt: no content needed
#[derive(Debug, Default)]
pub struct EnvironmentContent {
    /// The raw lockfile content (for BaseLockfile tier).
    pub lockfile_content: Option<String>,
    /// The raw Dockerfile content (for Dockerfile tier).
    pub dockerfile_content: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base_lockfile_hash_stable() {
        let tier = EnvironmentTier::BaseLockfile {
            base: "ozzydb/python:3.12".to_string(),
            lockfile: "uv.lock".to_string(),
        };
        let content = EnvironmentContent {
            lockfile_content: Some("polars==1.0\npyarrow==17.0\n".to_string()),
            ..Default::default()
        };

        let h1 = compute_env_hash(&tier, &content);
        let h2 = compute_env_hash(&tier, &content);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn test_base_lockfile_hash_changes_with_base() {
        let content = EnvironmentContent {
            lockfile_content: Some("polars==1.0\n".to_string()),
            ..Default::default()
        };

        let tier1 = EnvironmentTier::BaseLockfile {
            base: "ozzydb/python:3.12".to_string(),
            lockfile: "uv.lock".to_string(),
        };
        let tier2 = EnvironmentTier::BaseLockfile {
            base: "ozzydb/python:3.11".to_string(),
            lockfile: "uv.lock".to_string(),
        };

        let h1 = compute_env_hash(&tier1, &content);
        let h2 = compute_env_hash(&tier2, &content);
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_base_lockfile_hash_changes_with_lockfile_content() {
        let tier = EnvironmentTier::BaseLockfile {
            base: "ozzydb/python:3.12".to_string(),
            lockfile: "uv.lock".to_string(),
        };

        let content1 = EnvironmentContent {
            lockfile_content: Some("polars==1.0\n".to_string()),
            ..Default::default()
        };
        let content2 = EnvironmentContent {
            lockfile_content: Some("polars==2.0\n".to_string()),
            ..Default::default()
        };

        let h1 = compute_env_hash(&tier, &content1);
        let h2 = compute_env_hash(&tier, &content2);
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_dockerfile_hash_stable() {
        let tier = EnvironmentTier::Dockerfile {
            dockerfile: "Dockerfile".to_string(),
        };
        let content = EnvironmentContent {
            dockerfile_content: Some("FROM python:3.12\nRUN pip install polars\n".to_string()),
            ..Default::default()
        };

        let h1 = compute_env_hash(&tier, &content);
        let h2 = compute_env_hash(&tier, &content);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn test_dockerfile_hash_changes_with_content() {
        let tier = EnvironmentTier::Dockerfile {
            dockerfile: "Dockerfile".to_string(),
        };

        let c1 = EnvironmentContent {
            dockerfile_content: Some("FROM python:3.12\n".to_string()),
            ..Default::default()
        };
        let c2 = EnvironmentContent {
            dockerfile_content: Some("FROM python:3.11\n".to_string()),
            ..Default::default()
        };

        let h1 = compute_env_hash(&tier, &c1);
        let h2 = compute_env_hash(&tier, &c2);
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_prebuilt_returns_hash() {
        let tier = EnvironmentTier::Prebuilt {
            image: "ghcr.io/user/image:v1".to_string(),
        };
        let content = EnvironmentContent::default();

        let h = compute_env_hash(&tier, &content);
        assert_eq!(h.len(), 64);
        assert_eq!(h, ozzy_core::hash::blake3_hash(b"ghcr.io/user/image:v1"));
    }

    #[test]
    fn test_golden_base_lockfile_hash() {
        // blake3_hash_components(["ozzydb/python:3.12", "polars==1.0\n"])
        // = blake3("ozzydb/python:3.12\0polars==1.0\n")
        let tier = EnvironmentTier::BaseLockfile {
            base: "ozzydb/python:3.12".to_string(),
            lockfile: "uv.lock".to_string(),
        };
        let content = EnvironmentContent {
            lockfile_content: Some("polars==1.0\n".to_string()),
            ..Default::default()
        };

        let h = compute_env_hash(&tier, &content);
        let expected = blake3_hash_components(&["ozzydb/python:3.12", "polars==1.0\n"]);
        assert_eq!(h, expected);
    }

    #[test]
    fn test_golden_dockerfile_hash() {
        // blake3_hash("FROM python:3.12\n")
        let tier = EnvironmentTier::Dockerfile {
            dockerfile: "Dockerfile".to_string(),
        };
        let content = EnvironmentContent {
            dockerfile_content: Some("FROM python:3.12\n".to_string()),
            ..Default::default()
        };

        let h = compute_env_hash(&tier, &content);
        let expected = ozzy_core::hash::blake3_hash(b"FROM python:3.12\n");
        assert_eq!(h, expected);
    }
}
