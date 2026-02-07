//! Pull command for downloading commits from a remote registry.

use anyhow::{Context, Result};
use ozzy_core::Project;
use ozzy_core::commit;
use ozzy_core::error::Error as CoreError;
use ozzy_core::project::Commit;
use ozzy_core::registry::protocol::ListRefsResponse;
use ozzy_core::registry::RegistryClient;
use std::collections::{BTreeMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::remote::get_remote_url;
use super::shared::{checked_destination, load_credentials, sanitize_archive_relative_path};

fn prune_unlisted_files(dir: &Path, project_root: &Path, keep: &HashSet<PathBuf>) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            prune_unlisted_files(&path, project_root, keep)?;
            if std::fs::read_dir(&path)?.next().is_none() {
                std::fs::remove_dir(&path)?;
            }
            continue;
        }

        if file_type.is_file() {
            let rel = path
                .strip_prefix(project_root)
                .map_err(|e| anyhow::anyhow!("Failed to compute relative path: {}", e))?
                .to_path_buf();
            if !keep.contains(&rel) {
                std::fs::remove_file(&path)?;
            }
        }
    }

    Ok(())
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

fn is_workspace_dirty_since_last_commit(project: &Project) -> Result<bool> {
    let current_data_hashes: BTreeMap<String, String> = commit::collect_data_sources(project)?
        .into_values()
        .map(|ds| (ds.path, ds.hash))
        .collect();
    let current_transforms = commit::collect_transforms(project)?;

    let last_commit = match project.latest_commit() {
        Ok(commit) => commit,
        Err(CoreError::CommitNotFound(_)) => None,
        Err(err) => return Err(err.into()),
    };

    if let Some(last_commit) = last_commit {
        let committed_data_hashes: BTreeMap<String, String> = last_commit
            .data_sources
            .into_values()
            .map(|ds| (ds.path, ds.hash))
            .collect();
        Ok(current_data_hashes != committed_data_hashes
            || current_transforms != last_commit.transforms)
    } else {
        Ok(!current_data_hashes.is_empty() || !current_transforms.is_empty())
    }
}

fn validate_pulled_commit_json(commit_json: &[u8], expected_hash: &str) -> Result<Commit> {
    let commit: Commit =
        serde_json::from_slice(commit_json).context("Failed to parse commit.json from archive")?;

    if commit.hash != expected_hash {
        anyhow::bail!(
            "Pull archive commit hash mismatch: manifest={}, commit.json={}",
            expected_hash,
            commit.hash
        );
    }

    Ok(commit)
}

fn reconcile_staged_endpoints_after_pull(project: &Project) -> Result<()> {
    let staged_dir = project.ozzy_dir().join("staged_endpoints");
    if !staged_dir.exists() {
        return Ok(());
    }

    let has_entries = std::fs::read_dir(&staged_dir)?.next().is_some();
    if !has_entries {
        std::fs::remove_dir_all(&staged_dir)?;
        return Ok(());
    }

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut backup_dir = project
        .ozzy_dir()
        .join(format!("staged_endpoints.backup.{}", ts));
    let mut suffix = 0u32;
    while backup_dir.exists() {
        suffix += 1;
        backup_dir = project
            .ozzy_dir()
            .join(format!("staged_endpoints.backup.{}.{}", ts, suffix));
    }

    std::fs::rename(&staged_dir, &backup_dir)?;
    println!(
        "  Note: moved local staged endpoints to {}",
        backup_dir.display()
    );
    Ok(())
}

/// Pull from a remote registry.
pub async fn run(remote: Option<&str>, ref_name: Option<&str>, force: bool) -> Result<()> {
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
    // Pass ref name to server as-is (including refs/tags/ or refs/heads/ prefix)
    // so the server can resolve the correct type. Only strip the '@' shorthand.
    let server_ref = match requested_ref {
        "@latest" => "@latest",
        s => s.strip_prefix('@').unwrap_or(s),
    };
    // For local display, strip prefixes to get the bare name
    let display_ref = server_ref
        .strip_prefix("refs/heads/")
        .or_else(|| server_ref.strip_prefix("refs/tags/"))
        .unwrap_or(server_ref);

    let workspace_dirty = is_workspace_dirty_since_last_commit(&project)?;
    if workspace_dirty && !force {
        anyhow::bail!(
            "Refusing to pull: local data/ and transforms/ contain uncommitted changes. Commit them first or re-run with --force."
        );
    }
    if workspace_dirty && force {
        println!(
            "  Warning: --force set, local uncommitted data/transforms changes will be overwritten"
        );
    }

    // Get manifest first to show what will be downloaded
    let manifest = client
        .pull_manifest(owner, project_slug, Some(server_ref))
        .await?;

    println!(
        "Pulling commit {} ({} data sources, {} transforms)",
        manifest
            .commit_hash
            .get(..8)
            .unwrap_or(manifest.commit_hash.as_str()),
        manifest.data_hashes.len(),
        manifest.transform_hashes.len()
    );

    // Download the tar archive
    let tar_data = client.pull(owner, project_slug, Some(server_ref)).await?;

    // Extract the tar archive
    let cursor = std::io::Cursor::new(tar_data);
    let mut archive = tar::Archive::new(cursor);

    let mut commit_json_data: Option<Vec<u8>> = None;
    let mut extracted_files: Vec<(PathBuf, Vec<u8>)> = Vec::new();
    let mut keep_data_files: HashSet<PathBuf> = HashSet::new();
    let mut keep_transform_files: HashSet<PathBuf> = HashSet::new();

    for entry in archive.entries()? {
        let mut entry = entry?;
        let raw_path = entry.path()?.to_path_buf();
        let path = sanitize_archive_relative_path(&raw_path)?;

        // Read file content
        let mut content = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut content)?;

        // Capture commit.json for integrity verification and local storage
        if path.to_string_lossy() == "commit.json" {
            commit_json_data = Some(content);
            continue;
        }

        extracted_files.push((path, content));
    }

    let commit_json_data =
        commit_json_data.context("Pull archive is missing required file commit.json")?;
    let pulled_commit = validate_pulled_commit_json(&commit_json_data, &manifest.commit_hash)?;

    // Build expected hash maps for content verification
    let expected_data: std::collections::HashMap<String, String> = pulled_commit
        .data_sources
        .values()
        .map(|ds| (ds.path.clone(), ds.hash.clone()))
        .collect();
    let expected_transforms: std::collections::HashMap<String, String> = pulled_commit
        .transforms
        .values()
        .map(|t| (t.source_path.clone(), t.hash.clone()))
        .collect();

    // Create data and transforms directories
    std::fs::create_dir_all(project.root.join("data"))?;
    std::fs::create_dir_all(project.root.join("transforms"))?;
    let canonical_project_root = project.root.canonicalize()?;

    for (path, content) in extracted_files {
        // Verify content hashes against the commit metadata
        let rel = path.to_string_lossy().replace('\\', "/");
        if let Some(expected) = expected_data.get(&rel) {
            let actual = ozzy_core::hash::blake3_hash(&content);
            if actual != *expected {
                anyhow::bail!(
                    "Data hash mismatch for {}: expected {}, got {}",
                    rel,
                    expected,
                    actual
                );
            }
        }
        if let Some(expected) = expected_transforms.get(&rel) {
            let text = std::str::from_utf8(&content)
                .with_context(|| format!("Transform {} is not valid UTF-8", rel))?;
            let canonical = ozzy_core::canon::canonicalize_source(text);
            let actual = ozzy_core::hash::blake3_hash(canonical.as_bytes());
            if actual != *expected {
                anyhow::bail!(
                    "Transform hash mismatch for {}: expected {}, got {}",
                    rel,
                    expected,
                    actual
                );
            }
        }

        let dest_path = checked_destination(&project.root, &canonical_project_root, &path)?;

        let mut file = std::fs::File::create(&dest_path)?;
        file.write_all(&content)?;

        if path.starts_with("data") {
            keep_data_files.insert(path.clone());
        } else if path.starts_with("transforms") {
            keep_transform_files.insert(path.clone());
        }

        println!("  {}", path.display());
    }

    prune_unlisted_files(
        &project.data_dir(),
        &canonical_project_root,
        &keep_data_files,
    )?;
    prune_unlisted_files(
        &project.transforms_dir(),
        &canonical_project_root,
        &keep_transform_files,
    )?;

    // Store commit in .ozzy/commits/{hash}.json
    let commits_dir = project.commits_dir();
    std::fs::create_dir_all(&commits_dir)?;
    let commit_path = commits_dir.join(format!("{}.json", manifest.commit_hash));
    let mut file = std::fs::File::create(&commit_path)?;
    file.write_all(&commit_json_data)?;

    // Resolve whether this is a branch or tag ref so local refs stay consistent.
    let refs = client.list_refs(owner, project_slug).await.ok();
    let ref_full = resolve_local_ref_path(display_ref, refs.as_ref(), &project.config.refs.head);
    project.update_ref(&ref_full, &manifest.commit_hash)?;

    // Also update HEAD if we're pulling the branch that HEAD points to
    if ref_full == project.config.refs.head {
        project.update_ref(&project.config.refs.head, &manifest.commit_hash)?;
    }

    // Reconcile any pre-existing local endpoint staging so pulled state becomes
    // authoritative for subsequent commits.
    reconcile_staged_endpoints_after_pull(&project)?;

    println!();
    println!(
        "Pull complete. Updated ref '{}' -> {}",
        display_ref,
        manifest
            .commit_hash
            .get(..8)
            .unwrap_or(manifest.commit_hash.as_str())
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        is_workspace_dirty_since_last_commit, reconcile_staged_endpoints_after_pull,
        resolve_local_ref_path, sanitize_archive_relative_path, validate_pulled_commit_json,
    };
    use ozzy_core::commit as commit_lib;
    use ozzy_core::registry::protocol::{ListRefsResponse, RefInfo};
    use std::path::Path;
    use tempfile::tempdir;

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

    #[test]
    fn reconcile_staged_endpoints_moves_to_backup() {
        let dir = tempdir().unwrap();
        let project = ozzy_core::Project::init(dir.path(), "test", "user").unwrap();

        let staged_dir = project.ozzy_dir().join("staged_endpoints");
        std::fs::create_dir_all(&staged_dir).unwrap();
        std::fs::write(staged_dir.join("ep.deleted"), b"deleted\n").unwrap();

        reconcile_staged_endpoints_after_pull(&project).unwrap();
        assert!(!staged_dir.exists());

        let backup_count = std::fs::read_dir(project.ozzy_dir())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("staged_endpoints.backup.")
            })
            .count();
        assert_eq!(backup_count, 1);
    }

    #[test]
    fn workspace_dirty_true_without_commit() {
        let dir = tempdir().unwrap();
        let project = ozzy_core::Project::init(dir.path(), "test", "user").unwrap();

        std::fs::write(
            project.transforms_dir().join("t.py"),
            r#"
class ozzy:
    @staticmethod
    def transform(**kwargs):
        def decorator(fn):
            return fn
        return decorator

@ozzy.transform()
def t(inputs, params):
    return inputs["main"]
"#,
        )
        .unwrap();

        assert!(is_workspace_dirty_since_last_commit(&project).unwrap());
    }

    #[test]
    fn workspace_dirty_detects_transform_hash_changes() {
        let dir = tempdir().unwrap();
        let project = ozzy_core::Project::init(dir.path(), "test", "user").unwrap();
        let transform_path = project.transforms_dir().join("t.py");

        std::fs::write(
            &transform_path,
            r#"
class ozzy:
    @staticmethod
    def transform(**kwargs):
        def decorator(fn):
            return fn
        return decorator

@ozzy.transform()
def t(inputs, params):
    return inputs["main"]
"#,
        )
        .unwrap();

        let commit = commit_lib::create_commit(&project, "initial", "tester").unwrap();
        project.save_commit(&commit).unwrap();
        project
            .update_ref(&project.config.refs.head, &commit.hash)
            .unwrap();

        assert!(!is_workspace_dirty_since_last_commit(&project).unwrap());

        std::fs::write(
            &transform_path,
            r#"
class ozzy:
    @staticmethod
    def transform(**kwargs):
        def decorator(fn):
            return fn
        return decorator

@ozzy.transform()
def t(inputs, params):
    return inputs["main"].head(1)
"#,
        )
        .unwrap();

        assert!(is_workspace_dirty_since_last_commit(&project).unwrap());
    }

    #[test]
    fn validate_pulled_commit_json_requires_matching_hash() {
        let dir = tempdir().unwrap();
        let project = ozzy_core::Project::init(dir.path(), "test", "user").unwrap();

        let commit = commit_lib::create_commit(&project, "initial", "tester").unwrap();
        let commit_json = serde_json::to_vec(&commit).unwrap();

        assert!(validate_pulled_commit_json(&commit_json, &commit.hash).is_ok());
        assert!(validate_pulled_commit_json(&commit_json, "different-hash").is_err());
    }
}
