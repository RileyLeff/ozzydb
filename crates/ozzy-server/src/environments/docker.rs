//! Docker-based environment builder.
//!
//! Builds Docker images from published environment definitions using
//! `docker build`. Images are tagged as `ozzydb-env:{env_hash}` for local use.

use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result};
use tokio::process::Command;

use crate::config::ComputeConfig;
use crate::db::Database;

use super::{BuiltEnvironment, generate_dockerfile, image_tag};

use ozzy_core::toml_spec::PublishedEnvironmentDef;

use super::hash::compute_env_hash;

/// Build (or resolve) an environment image from its tier definition.
///
/// - For BaseLockfile: generates a Dockerfile, runs `docker build`, records in DB.
/// - For Dockerfile: uses the provided Dockerfile content, runs `docker build`.
/// - For Prebuilt: returns the image reference directly (no build).
///
/// Returns early if the environment is already built (dedup via DB lookup).
pub async fn build_environment(
    db: &Database,
    config: &ComputeConfig,
    env_name: &str,
    definition: &PublishedEnvironmentDef,
) -> Result<BuiltEnvironment> {
    match definition {
        PublishedEnvironmentDef::Prebuilt { image } => {
            // No build needed — use the image reference directly.
            // We still record it in the DB for tracking.
            let env_hash = compute_env_hash(definition);
            if let Some(existing) = db.get_environment_image(&env_hash).await? {
                return Ok(BuiltEnvironment {
                    env_hash,
                    image_ref: existing.image_ref,
                    build_type: "prebuilt".to_string(),
                    base_image: None,
                    build_duration_ms: None,
                    build_log: None,
                });
            }

            db.insert_environment_image(&env_hash, image, "prebuilt", None)
                .await?;
            db.mark_environment_built(&env_hash, None, 0).await?;

            Ok(BuiltEnvironment {
                env_hash,
                image_ref: image.clone(),
                build_type: "prebuilt".to_string(),
                base_image: None,
                build_duration_ms: Some(0),
                build_log: None,
            })
        }

        PublishedEnvironmentDef::BaseLockfile {
            base,
            installer,
            lockfile_content,
        } => {
            let env_hash = compute_env_hash(definition);

            // Check if already built
            if let Some(existing) = db.get_environment_image(&env_hash).await? {
                if existing.built_at.is_some() {
                    tracing::info!(
                        "Environment '{}' already built: {}",
                        env_name,
                        existing.image_ref
                    );
                    return Ok(BuiltEnvironment {
                        env_hash,
                        image_ref: existing.image_ref,
                        build_type: "base_lockfile".to_string(),
                        base_image: Some(base.clone()),
                        build_duration_ms: existing.build_duration_ms,
                        build_log: None,
                    });
                }
            }

            let tag = image_tag(&env_hash);
            let dockerfile_content = generate_dockerfile(base, installer)
                .map_err(|e| anyhow::anyhow!("Invalid environment definition: {}", e))?;
            let lockfile_bytes = lockfile_content.as_bytes();

            // Record pending build
            // Use upsert pattern: ignore conflict if another task already inserted
            if db.get_environment_image(&env_hash).await?.is_none() {
                db.insert_environment_image(&env_hash, &tag, "base_lockfile", Some(base))
                    .await?;
            }

            let start = Instant::now();
            let build_log = docker_build(&dockerfile_content, lockfile_bytes, &tag, config).await?;
            let duration_ms: i32 = start.elapsed().as_millis().try_into().unwrap_or(i32::MAX);

            db.mark_environment_built(&env_hash, None, duration_ms)
                .await?;

            tracing::info!(
                "Built environment '{}' ({}) in {}ms",
                env_name,
                tag,
                duration_ms
            );

            Ok(BuiltEnvironment {
                env_hash,
                image_ref: tag,
                build_type: "base_lockfile".to_string(),
                base_image: Some(base.clone()),
                build_duration_ms: Some(duration_ms),
                build_log: Some(build_log),
            })
        }

        PublishedEnvironmentDef::Dockerfile { dockerfile_content } => {
            let env_hash = compute_env_hash(definition);

            // Check if already built
            if let Some(existing) = db.get_environment_image(&env_hash).await? {
                if existing.built_at.is_some() {
                    tracing::info!(
                        "Environment '{}' already built: {}",
                        env_name,
                        existing.image_ref
                    );
                    return Ok(BuiltEnvironment {
                        env_hash,
                        image_ref: existing.image_ref,
                        build_type: "dockerfile".to_string(),
                        base_image: None,
                        build_duration_ms: existing.build_duration_ms,
                        build_log: None,
                    });
                }
            }

            let tag = image_tag(&env_hash);

            // Record pending build
            if db.get_environment_image(&env_hash).await?.is_none() {
                db.insert_environment_image(&env_hash, &tag, "dockerfile", None)
                    .await?;
            }

            let start = Instant::now();
            let build_log = docker_build(dockerfile_content, &[], &tag, config).await?;
            let duration_ms: i32 = start.elapsed().as_millis().try_into().unwrap_or(i32::MAX);

            db.mark_environment_built(&env_hash, None, duration_ms)
                .await?;

            tracing::info!(
                "Built environment '{}' ({}) in {}ms",
                env_name,
                tag,
                duration_ms
            );

            Ok(BuiltEnvironment {
                env_hash,
                image_ref: tag,
                build_type: "dockerfile".to_string(),
                base_image: None,
                build_duration_ms: Some(duration_ms),
                build_log: Some(build_log),
            })
        }
    }
}

/// Run `docker build` with a generated Dockerfile and optional lockfile.
///
/// Creates a temporary build context directory, writes the Dockerfile and lockfile,
/// then runs `docker build -t <tag> .` inside it.
///
/// Returns the combined stdout+stderr as the build log.
async fn docker_build(
    dockerfile_content: &str,
    lockfile_bytes: &[u8],
    tag: &str,
    _config: &ComputeConfig,
) -> Result<String> {
    // Create a temporary build context directory
    let build_dir = tempfile::tempdir().context("Failed to create temp build directory")?;
    let build_path = build_dir.path();

    // Write Dockerfile
    tokio::fs::write(build_path.join("Dockerfile"), dockerfile_content)
        .await
        .context("Failed to write Dockerfile")?;

    // Write lockfile (if provided)
    if !lockfile_bytes.is_empty() {
        tokio::fs::write(build_path.join("lockfile"), lockfile_bytes)
            .await
            .context("Failed to write lockfile")?;
    }

    run_docker_build(build_path, tag).await
}

/// Execute the `docker build` command.
async fn run_docker_build(build_context: &Path, tag: &str) -> Result<String> {
    let output = Command::new("docker")
        .args(["build", "-t", tag, "."])
        .current_dir(build_context)
        .output()
        .await
        .context("Failed to execute docker build")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}{}", stdout, stderr);

    if !output.status.success() {
        anyhow::bail!(
            "docker build failed (exit {}): {}",
            output.status.code().unwrap_or(-1),
            combined
        );
    }

    Ok(combined)
}

/// Check if a Docker image exists locally.
pub async fn image_exists(tag: &str) -> Result<bool> {
    let output = Command::new("docker")
        .args(["image", "inspect", tag])
        .output()
        .await
        .context("Failed to execute docker image inspect")?;

    Ok(output.status.success())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::environments::build_type_str;
    use ozzy_core::toml_spec::EnvironmentTier;

    #[test]
    fn test_image_tag_format() {
        let tag = image_tag("abc123def456");
        assert_eq!(tag, "ozzydb-env:abc123def456");
    }

    #[test]
    fn test_build_type_str_values() {
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
