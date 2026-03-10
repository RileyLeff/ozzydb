//! Published environment hash computation.
//!
//! Environment realization is keyed by the fully resolved published definition,
//! not by authored `ozzy.toml` path references. This keeps `EnvironmentVersion`
//! identity separate from provider-specific image realization while still
//! producing stable content-addressed build keys.

use ozzy_core::hash::blake3_hash_components;
use ozzy_core::toml_spec::PublishedEnvironmentDef;

/// Compute the realization hash for a published environment definition.
pub fn compute_env_hash(definition: &PublishedEnvironmentDef) -> String {
    match definition {
        PublishedEnvironmentDef::BaseLockfile {
            base,
            installer,
            lockfile_content,
        } => blake3_hash_components(&[base, installer.as_identity_str(), lockfile_content]),
        PublishedEnvironmentDef::Dockerfile { dockerfile_content } => {
            blake3_hash_components(&[dockerfile_content])
        }
        PublishedEnvironmentDef::Prebuilt { image } => {
            ozzy_core::hash::blake3_hash(image.as_bytes())
        }
    }
}

/// Compute the lockfile hash contribution for transform hashing.
pub fn compute_lockfile_hash(definition: &PublishedEnvironmentDef) -> String {
    match definition {
        PublishedEnvironmentDef::BaseLockfile {
            lockfile_content, ..
        } => ozzy_core::hash::blake3_hash(lockfile_content.as_bytes()),
        PublishedEnvironmentDef::Dockerfile { .. } | PublishedEnvironmentDef::Prebuilt { .. } => {
            ozzy_core::hash::blake3_hash(b"")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base_lockfile_hash_stable() {
        let definition = PublishedEnvironmentDef::BaseLockfile {
            base: "ozzydb/python:3.12".to_string(),
            installer: ozzy_core::toml_spec::BaseLockfileInstaller::PipRequirements,
            lockfile_content: "polars==1.0\npyarrow==17.0\n".to_string(),
        };

        let h1 = compute_env_hash(&definition);
        let h2 = compute_env_hash(&definition);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn test_base_lockfile_hash_ignores_authored_path() {
        let d1 = PublishedEnvironmentDef::BaseLockfile {
            base: "ozzydb/python:3.12".to_string(),
            installer: ozzy_core::toml_spec::BaseLockfileInstaller::PipRequirements,
            lockfile_content: "polars==1.0\n".to_string(),
        };
        let d2 = PublishedEnvironmentDef::BaseLockfile {
            base: "ozzydb/python:3.12".to_string(),
            installer: ozzy_core::toml_spec::BaseLockfileInstaller::PipRequirements,
            lockfile_content: "polars==1.0\n".to_string(),
        };

        assert_eq!(compute_env_hash(&d1), compute_env_hash(&d2));
    }

    #[test]
    fn test_dockerfile_hash_stable() {
        let definition = PublishedEnvironmentDef::Dockerfile {
            dockerfile_content: "FROM python:3.12\nRUN pip install polars\n".to_string(),
        };

        let h1 = compute_env_hash(&definition);
        let h2 = compute_env_hash(&definition);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn test_prebuilt_returns_hash() {
        let definition = PublishedEnvironmentDef::Prebuilt {
            image: "ghcr.io/user/image:v1".to_string(),
        };

        let h = compute_env_hash(&definition);
        assert_eq!(h.len(), 64);
        assert_eq!(h, ozzy_core::hash::blake3_hash(b"ghcr.io/user/image:v1"));
    }

    #[test]
    fn test_lockfile_hash_for_base_lockfile() {
        let definition = PublishedEnvironmentDef::BaseLockfile {
            base: "ozzydb/python:3.12".to_string(),
            installer: ozzy_core::toml_spec::BaseLockfileInstaller::PipRequirements,
            lockfile_content: "polars==1.0\n".to_string(),
        };

        assert_eq!(
            compute_lockfile_hash(&definition),
            ozzy_core::hash::blake3_hash(b"polars==1.0\n")
        );
    }

    #[test]
    fn test_lockfile_hash_empty_for_non_lockfile_tiers() {
        let dockerfile = PublishedEnvironmentDef::Dockerfile {
            dockerfile_content: "FROM python:3.12\n".to_string(),
        };
        let prebuilt = PublishedEnvironmentDef::Prebuilt {
            image: "ghcr.io/user/image:v1".to_string(),
        };

        let expected = ozzy_core::hash::blake3_hash(b"");
        assert_eq!(compute_lockfile_hash(&dockerfile), expected);
        assert_eq!(compute_lockfile_hash(&prebuilt), expected);
    }
}
