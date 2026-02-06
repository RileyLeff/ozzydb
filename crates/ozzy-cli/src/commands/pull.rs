//! Pull command for downloading commits from a remote registry.

use anyhow::{Context, Result};
use ozzy_core::Project;
use ozzy_core::registry::protocol::ListRefsResponse;
use ozzy_core::registry::{CredentialsFile, RegistryClient};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use super::remote::get_remote_url;

/// Load credentials from config.
fn load_credentials() -> Result<CredentialsFile> {
    let path = dirs::config_dir()
        .context("Could not determine config directory")?
        .join("ozzy")
        .join("credentials.json");

    if path.exists() {
        let content = std::fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&content)?)
    } else {
        Ok(CredentialsFile::default())
    }
}

fn sanitize_archive_relative_path(path: &Path) -> Result<PathBuf> {
    let mut sanitized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => sanitized.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                anyhow::bail!("Refusing unsafe archive path: {}", path.display());
            }
        }
    }

    if sanitized.as_os_str().is_empty() {
        anyhow::bail!("Refusing empty archive path");
    }

    Ok(sanitized)
}

fn checked_destination(base: &Path, canonical_base: &Path, rel: &Path) -> Result<PathBuf> {
    let dest = base.join(rel);

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
        let canonical_parent = parent.canonicalize()?;
        if !canonical_parent.starts_with(canonical_base) {
            anyhow::bail!(
                "Archive extraction escaped destination root: {}",
                rel.display()
            );
        }
    }

    if dest.exists() {
        let canonical_dest = dest.canonicalize()?;
        if !canonical_dest.starts_with(canonical_base) {
            anyhow::bail!(
                "Archive extraction escaped destination root: {}",
                rel.display()
            );
        }
    }

    Ok(dest)
}

fn resolve_local_ref_path(
    normalized_ref: &str,
    refs: Option<&ListRefsResponse>,
    head_ref: &str,
) -> String {
    if normalized_ref == "@latest" {
        return head_ref.to_string();
    }

    let is_tag_only = refs
        .map(|r| {
            let tag_match = r.tags.iter().any(|t| t.name == normalized_ref);
            let branch_match = r.branches.iter().any(|b| b.name == normalized_ref);
            tag_match && !branch_match
        })
        .unwrap_or(false);

    if is_tag_only {
        format!("refs/tags/{}", normalized_ref)
    } else {
        format!("refs/heads/{}", normalized_ref)
    }
}

/// Pull from a remote registry.
pub async fn run(remote: Option<&str>, ref_name: Option<&str>) -> Result<()> {
    let project = Project::find_current()?;

    // Get remote URL
    let (remote_name, remote_url) = get_remote_url(&project, remote)?;

    println!("Pulling from {} ({})...", remote_name, remote_url);

    // Get credentials (optional for public projects)
    let creds = load_credentials().ok();
    let token = creds
        .as_ref()
        .and_then(|c| c.get(&remote_url))
        .map(|c| c.access_token.as_str());

    let client = if let Some(t) = token {
        RegistryClient::with_token(&remote_url, t)
    } else {
        RegistryClient::new(&remote_url)
    };

    let owner = &project.config.project.owner;
    let project_slug = &project.config.project.name;
    let requested_ref = ref_name.unwrap_or("main");
    let normalized_ref = match requested_ref {
        "@latest" => "@latest",
        s => s
            .strip_prefix("refs/heads/")
            .or_else(|| s.strip_prefix("refs/tags/"))
            .or_else(|| s.strip_prefix('@'))
            .unwrap_or(s),
    };

    // Get manifest first to show what will be downloaded
    let manifest = client
        .pull_manifest(owner, project_slug, Some(normalized_ref))
        .await?;

    println!(
        "Pulling commit {} ({} data sources, {} transforms)",
        &manifest.commit_hash[..8],
        manifest.data_hashes.len(),
        manifest.transform_hashes.len()
    );

    // Download the tar archive
    let tar_data = client
        .pull(owner, project_slug, Some(normalized_ref))
        .await?;

    // Extract the tar archive
    let cursor = std::io::Cursor::new(tar_data);
    let mut archive = tar::Archive::new(cursor);

    // Create data and transforms directories
    std::fs::create_dir_all(project.root.join("data"))?;
    std::fs::create_dir_all(project.root.join("transforms"))?;
    let canonical_project_root = project.root.canonicalize()?;

    let mut commit_json_data: Option<Vec<u8>> = None;

    for entry in archive.entries()? {
        let mut entry = entry?;
        let raw_path = entry.path()?.to_path_buf();
        let path = sanitize_archive_relative_path(&raw_path)?;

        // Read file content
        let mut content = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut content)?;

        // Capture commit.json for storing locally
        if path.to_string_lossy() == "commit.json" {
            commit_json_data = Some(content.clone());
        }

        let dest_path = checked_destination(&project.root, &canonical_project_root, &path)?;

        let mut file = std::fs::File::create(&dest_path)?;
        file.write_all(&content)?;

        println!("  {}", path.display());
    }

    // Store commit in .ozzy/commits/{hash}.json
    if let Some(commit_data) = &commit_json_data {
        let commits_dir = project.commits_dir();
        std::fs::create_dir_all(&commits_dir)?;
        let commit_path = commits_dir.join(format!("{}.json", manifest.commit_hash));
        let mut file = std::fs::File::create(&commit_path)?;
        file.write_all(commit_data)?;
    }

    // Resolve whether this is a branch or tag ref so local refs stay consistent.
    let refs = client.list_refs(owner, project_slug).await.ok();
    let ref_full = resolve_local_ref_path(normalized_ref, refs.as_ref(), &project.config.refs.head);
    project.update_ref(&ref_full, &manifest.commit_hash)?;

    // Also update HEAD if we're pulling the default branch
    if ref_full == project.config.refs.head || normalized_ref == "main" {
        project.update_ref(&project.config.refs.head, &manifest.commit_hash)?;
    }

    println!();
    println!(
        "Pull complete. Updated ref '{}' -> {}",
        normalized_ref,
        &manifest.commit_hash[..8]
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{resolve_local_ref_path, sanitize_archive_relative_path};
    use ozzy_core::registry::protocol::{ListRefsResponse, RefInfo};
    use std::path::Path;

    #[test]
    fn sanitize_archive_path_rejects_traversal() {
        assert!(sanitize_archive_relative_path(Path::new("../etc/passwd")).is_err());
        assert!(sanitize_archive_relative_path(Path::new("/absolute/path")).is_err());
    }

    #[test]
    fn sanitize_archive_path_allows_relative_paths() {
        let p = sanitize_archive_relative_path(Path::new("data/raw.parquet")).unwrap();
        assert_eq!(p.to_string_lossy(), "data/raw.parquet");
    }

    #[test]
    fn resolve_ref_path_uses_tag_for_tag_only_name() {
        let refs = ListRefsResponse {
            branches: vec![RefInfo {
                name: "main".to_string(),
                ref_type: "branch".to_string(),
                commit_hash: "a".to_string(),
                updated_at: "now".to_string(),
            }],
            tags: vec![RefInfo {
                name: "v1.0.0".to_string(),
                ref_type: "tag".to_string(),
                commit_hash: "b".to_string(),
                updated_at: "now".to_string(),
            }],
        };

        let resolved = resolve_local_ref_path("v1.0.0", Some(&refs), "refs/heads/main");
        assert_eq!(resolved, "refs/tags/v1.0.0");
    }
}
