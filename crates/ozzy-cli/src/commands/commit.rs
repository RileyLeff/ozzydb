use anyhow::Result;
use ozzy_core::project::Endpoint;
use ozzy_core::{commit as commit_lib, Project};
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

    // Add staged endpoints to the commit
    let staged_dir = project.ozzy_dir().join("staged_endpoints");
    if staged_dir.exists() {
        for entry in fs::read_dir(&staged_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map(|e| e == "json").unwrap_or(false) {
                let content = fs::read_to_string(&path)?;
                let endpoint: Endpoint = serde_json::from_str(&content)?;
                commit.endpoints.insert(endpoint.name.clone(), endpoint);
            }
        }
    }

    // Recompute hash with endpoints included
    let commit_content = serde_json::json!({
        "parent_hashes": commit.parent_hashes,
        "data_sources": commit.data_sources,
        "transforms": commit.transforms,
        "endpoints": commit.endpoints,
        "author": commit.author,
        "message": commit.message,
    });
    commit.hash = ozzy_core::hash::blake3_hash(commit_content.to_string().as_bytes());

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
            if entry.path().extension().map(|e| e == "json").unwrap_or(false) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
