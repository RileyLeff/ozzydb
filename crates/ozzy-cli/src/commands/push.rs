//! Push command for uploading commits to a remote registry.

use anyhow::{Context, Result};
use ozzy_core::Project;
use ozzy_core::registry::RegistryClient;
use std::collections::HashMap;

use super::remote::get_remote_url;
use super::shared::load_credentials;

/// Push current commit to a remote registry.
pub async fn run(message: Option<&str>, remote: &str) -> Result<()> {
    // `ozzy push -m ...` means "commit (if needed) then push", without mutating
    // an already-hashed commit payload in-flight.
    if let Some(msg) = message {
        super::commit::create(Some(msg)).await?;
    }

    let project = Project::find_current()?;

    // Get remote URL
    let (remote_name, remote_url) = get_remote_url(&project, Some(remote))?;

    println!("Pushing to {} ({})...", remote_name, remote_url);

    // Get credentials
    let creds = load_credentials()?;
    let registry_creds = creds
        .get(&remote_url)
        .context("Not logged in to this registry. Run 'ozzy auth login' first.")?;

    let client = RegistryClient::with_token(&remote_url, &registry_creds.access_token);

    // Parse remote URL to get owner/project
    // Expected format: https://registry.example.com/owner/project or just the base URL
    // For now, use project config for owner/slug
    let owner = &project.config.project.owner;
    let project_slug = &project.config.project.name;

    // Load latest commit
    let commit_hash = project
        .head_commit()?
        .context("No commits found. Create a commit first with 'ozzy commit'")?;
    let commit = project.load_commit(&commit_hash)?;

    // Collect data files
    let mut data_files: HashMap<String, Vec<u8>> = HashMap::new();
    for (name, ds) in &commit.data_sources {
        let path = project.root.join(&ds.path);
        if path.exists() {
            let content = std::fs::read(&path)?;
            data_files.insert(format!("{}.parquet", name), content);
        }
    }

    // Collect transform files and lockfiles
    let mut transform_files: HashMap<String, Vec<u8>> = HashMap::new();
    let mut lockfiles: HashMap<String, Vec<u8>> = HashMap::new();
    let mut seen_lockfile_hashes = std::collections::HashSet::new();
    for (name, t) in &commit.transforms {
        let path = project.root.join(&t.source_path);
        if path.exists() {
            let content = std::fs::read(&path)?;
            transform_files.insert(format!("{}.py", name), content);
        }

        // Collect lockfile if it exists and hasn't been collected yet
        if !t.lockfile_hash.is_empty() && seen_lockfile_hashes.insert(t.lockfile_hash.clone()) {
            if let Some(parent) = std::path::Path::new(&t.source_path).parent() {
                let lockfile_path = project.root.join(parent).join("uv.lock");
                if lockfile_path.exists() {
                    let content = std::fs::read(&lockfile_path)?;
                    lockfiles.insert(format!("{}.lock", t.lockfile_hash), content);
                }
            }
        }
    }

    // Check what content already exists
    let mut data_hashes: HashMap<String, String> = HashMap::new();
    for (name, ds) in &commit.data_sources {
        data_hashes.insert(name.clone(), ds.hash.clone());
    }
    let mut transform_hashes: HashMap<String, String> = HashMap::new();
    for (name, t) in &commit.transforms {
        transform_hashes.insert(name.clone(), t.hash.clone());
    }

    let check_response = client
        .check_content(owner, project_slug, &data_hashes, &transform_hashes)
        .await?;

    // Only upload missing content
    let data_to_upload: HashMap<String, Vec<u8>> = data_files
        .into_iter()
        .filter(|(name, _)| {
            let base_name = name.strip_suffix(".parquet").unwrap_or(name);
            check_response.missing_data.contains(&base_name.to_string())
        })
        .collect();

    let transforms_to_upload: HashMap<String, Vec<u8>> = transform_files
        .into_iter()
        .filter(|(name, _)| {
            let base_name = name.strip_suffix(".py").unwrap_or(name);
            check_response
                .missing_transforms
                .contains(&base_name.to_string())
        })
        .collect();

    println!(
        "Uploading {} data files, {} transform files",
        data_to_upload.len(),
        transforms_to_upload.len()
    );

    // Build commit JSON
    let mut commit_json = serde_json::to_value(&commit)?;
    commit_json["ref"] = serde_json::Value::String(project.config.refs.head.clone());

    let mut tag_map = serde_json::Map::new();
    let tags_dir = project.ozzy_dir().join("refs").join("tags");
    if tags_dir.exists() {
        for entry in std::fs::read_dir(&tags_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                let tag_name = entry.file_name().to_string_lossy().to_string();
                let commit_hash = std::fs::read_to_string(entry.path())?.trim().to_string();
                if !tag_name.is_empty() && !commit_hash.is_empty() {
                    tag_map.insert(tag_name, serde_json::Value::String(commit_hash));
                }
            }
        }
    }
    commit_json["tags"] = serde_json::Value::Object(tag_map);

    // Push
    let push_response = client
        .push(
            owner,
            project_slug,
            &commit_json,
            &data_to_upload,
            &transforms_to_upload,
            &lockfiles,
        )
        .await?;

    println!();
    println!(
        "Pushed commit {} to {}",
        &push_response.commit_hash[..8],
        push_response.ref_name
    );
    println!(
        "  {} new data files, {} new transforms",
        push_response.new_data_count, push_response.new_transform_count
    );

    Ok(())
}
