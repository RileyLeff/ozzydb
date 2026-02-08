//! Docker image management for transform execution.
//!
//! One image per unique lockfile hash: `ozzy-env:{lockfile_hash[:12]}`.
//! Transforms without a lockfile use `ozzy-env:base` (polars + pyarrow only).

use anyhow::{Context, Result};
use std::path::Path;
use tokio::process::Command;

/// Check if a Docker image exists locally.
async fn image_exists(tag: &str) -> bool {
    Command::new("docker")
        .args(["image", "inspect", tag])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Build a Docker image from a Dockerfile in a context directory.
async fn docker_build(tag: &str, context_dir: &Path) -> Result<()> {
    let output = Command::new("docker")
        .args(["build", "-t", tag, "."])
        .current_dir(context_dir)
        .output()
        .await
        .context("Failed to run docker build")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("docker build failed for {}: {}", tag, stderr);
    }

    Ok(())
}

/// Ensure the base image (`ozzy-env:base`) exists.
///
/// This image has Python 3.12, polars, and pyarrow pre-installed.
/// Used for transforms without a lockfile.
pub async fn ensure_base_image() -> Result<String> {
    let tag = "ozzy-env:base";

    if image_exists(tag).await {
        return Ok(tag.to_string());
    }

    tracing::info!("Building base compute image: {}", tag);

    let tmp_dir = tempfile::tempdir().context("Failed to create temp dir for Docker build")?;

    let dockerfile = r#"FROM python:3.12-slim
RUN pip install --no-cache-dir polars pyarrow
RUN useradd -m ozzy
USER ozzy
WORKDIR /work
"#;

    tokio::fs::write(tmp_dir.path().join("Dockerfile"), dockerfile)
        .await
        .context("Failed to write Dockerfile")?;

    docker_build(tag, tmp_dir.path()).await?;

    tracing::info!("Built base compute image: {}", tag);
    Ok(tag.to_string())
}

/// Ensure a lockfile-specific image exists.
///
/// Parses the lockfile content to extract `name==version` requirements,
/// then builds `ozzy-env:{lockfile_hash[:12]}` with those packages installed.
pub async fn ensure_lockfile_image(
    lockfile_hash: &str,
    lockfile_content: &str,
) -> Result<String> {
    let short_hash = lockfile_hash.get(..12).unwrap_or(lockfile_hash);
    let tag = format!("ozzy-env:{}", short_hash);

    if image_exists(&tag).await {
        return Ok(tag);
    }

    tracing::info!("Building compute image: {}", tag);

    let requirements = ozzy_core::runtime::parse_lockfile_requirements(lockfile_content)?;
    if requirements.is_empty() {
        // No packages in lockfile — fall back to base image.
        return ensure_base_image().await;
    }

    let tmp_dir = tempfile::tempdir().context("Failed to create temp dir for Docker build")?;

    let requirements_txt = requirements.join("\n");
    tokio::fs::write(tmp_dir.path().join("requirements.txt"), &requirements_txt)
        .await
        .context("Failed to write requirements.txt")?;

    let dockerfile = r#"FROM python:3.12-slim
COPY requirements.txt /tmp/requirements.txt
RUN pip install --no-cache-dir -r /tmp/requirements.txt
RUN useradd -m ozzy
USER ozzy
WORKDIR /work
"#;

    tokio::fs::write(tmp_dir.path().join("Dockerfile"), dockerfile)
        .await
        .context("Failed to write Dockerfile")?;

    docker_build(&tag, tmp_dir.path()).await?;

    tracing::info!("Built compute image: {}", tag);
    Ok(tag)
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_short_hash_truncation() {
        let hash = "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";
        let short = hash.get(..12).unwrap_or(hash);
        assert_eq!(short, "abcdef123456");
        assert_eq!(format!("ozzy-env:{}", short), "ozzy-env:abcdef123456");
    }

    #[test]
    fn test_short_hash_short_input() {
        let hash = "abc";
        let short = hash.get(..12).unwrap_or(hash);
        assert_eq!(short, "abc");
    }
}
