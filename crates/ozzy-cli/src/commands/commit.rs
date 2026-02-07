use anyhow::Result;
use ozzy_core::project::Endpoint;
use ozzy_core::{Project, commit as commit_lib};
use std::env;
use std::fs;

pub async fn create(message: Option<&str>) -> Result<()> {
    let mut project = Project::find_current()?;

    // Check for changes
    if !commit_lib::has_changes(&project)? && !has_staged_endpoints(&project)? {
        println!("Nothing to commit.");
        return Ok(());
    }

    // Get author
    let author = env::var("USER")
        .or_else(|_| env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".to_string());

    // Get or prompt for message
    let message = message.unwrap_or("Update project");

    // Build commit (this collects data sources and transforms)
    let mut commit = commit_lib::create_commit(&project, message, &author)?;

    // Apply staged endpoint deletions/additions to the commit.
    // Read directory entries once to avoid TOCTOU race between two read_dir calls.
    let staged_dir = project.ozzy_dir().join("staged_endpoints");
    if staged_dir.exists() {
        let mut deletions = Vec::new();
        let mut additions = Vec::new();

        for entry in fs::read_dir(&staged_dir)? {
            let entry = entry?;
            let path = entry.path();
            match path.extension().and_then(|e| e.to_str()) {
                Some("deleted") => deletions.push(path),
                Some("json") => additions.push(path),
                _ => {}
            }
        }

        // Apply deletions first, then additions (so re-creating an endpoint works)
        for path in &deletions {
            if let Some(stem) = path.file_stem() {
                let endpoint_name = stem.to_string_lossy().to_string();
                commit.endpoints.remove(&endpoint_name);
            }
        }
        for path in &additions {
            let content = fs::read_to_string(path)?;
            let endpoint: Endpoint = serde_json::from_str(&content)?;
            commit.endpoints.insert(endpoint.name.clone(), endpoint);
        }
    }

    // Recompute hash with endpoints included
    commit.hash = commit_lib::compute_commit_hash(&commit);

    // Save commit
    project.save_commit(&commit)?;

    // Update HEAD ref
    project.update_ref(&project.config.refs.head, &commit.hash)?;

    // Clean up staged endpoints
    if staged_dir.exists() {
        fs::remove_dir_all(&staged_dir)?;
    }

    // Clear workspace staged files
    project.config.workspace.staged_data.clear();
    project.config.workspace.staged_transforms.clear();
    project.save_config()?;

    println!("Committed: {}", &commit.hash[..12]);
    println!();
    println!("  Author: {}", commit.author);
    println!("  Message: {}", commit.message);
    println!("  Data sources: {}", commit.data_sources.len());
    println!("  Transforms: {}", commit.transforms.len());
    println!("  Endpoints: {}", commit.endpoints.len());

    Ok(())
}

fn has_staged_endpoints(project: &Project) -> Result<bool> {
    let staged_dir = project.ozzy_dir().join("staged_endpoints");
    if staged_dir.exists() {
        for entry in fs::read_dir(&staged_dir)? {
            let entry = entry?;
            if entry
                .path()
                .extension()
                .map(|e| e == "json" || e == "deleted")
                .unwrap_or(false)
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
