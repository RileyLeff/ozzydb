//! Tag command for creating version tags.

use anyhow::{Context, Result};
use ozzy_core::Project;

/// Create a tag pointing to the current HEAD commit.
pub async fn create(name: &str, message: Option<&str>) -> Result<()> {
    let project = Project::find_current()?;

    // Validate tag name
    ozzy_core::validate_safe_name(name)?;

    // Check if tag already exists
    let tag_path = project.ozzy_dir().join("refs").join("tags").join(name);
    if tag_path.exists() {
        anyhow::bail!("Tag '{}' already exists", name);
    }

    // Get current HEAD commit
    let head_ref = &project.config.refs.head;
    let commit_hash = project
        .resolve_ref(head_ref)?
        .context("No commits found. Create a commit first.")?;

    // Create tags directory if needed
    let tags_dir = project.ozzy_dir().join("refs").join("tags");
    std::fs::create_dir_all(&tags_dir)?;

    // Write tag file
    std::fs::write(&tag_path, format!("{}\n", commit_hash))?;

    println!(
        "Created tag '{}' at {}",
        name,
        &commit_hash[..8.min(commit_hash.len())]
    );

    if let Some(msg) = message {
        println!("  Message: {}", msg);
        // In a full implementation, we'd store annotated tag info
    }

    Ok(())
}

/// List all tags.
pub async fn list() -> Result<()> {
    let project = Project::find_current()?;

    let tags_dir = project.ozzy_dir().join("refs").join("tags");

    if !tags_dir.exists() {
        println!("No tags found.");
        return Ok(());
    }

    let mut tags: Vec<(String, String)> = Vec::new();

    for entry in std::fs::read_dir(&tags_dir)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            let name = entry.file_name().to_string_lossy().to_string();
            let hash = std::fs::read_to_string(entry.path())?.trim().to_string();
            tags.push((name, hash));
        }
    }

    if tags.is_empty() {
        println!("No tags found.");
        return Ok(());
    }

    tags.sort_by(|a, b| a.0.cmp(&b.0));

    println!("Tags:");
    for (name, hash) in tags {
        println!("  {} -> {}", name, &hash[..8.min(hash.len())]);
    }

    Ok(())
}

/// Delete a tag.
pub async fn delete(name: &str) -> Result<()> {
    let project = Project::find_current()?;

    let tag_path = project.ozzy_dir().join("refs").join("tags").join(name);

    if !tag_path.exists() {
        anyhow::bail!("Tag '{}' not found", name);
    }

    std::fs::remove_file(&tag_path)?;

    println!("Deleted tag '{}'", name);

    Ok(())
}

/// Show details of a specific tag.
pub async fn show(name: &str) -> Result<()> {
    let project = Project::find_current()?;

    let tag_path = project.ozzy_dir().join("refs").join("tags").join(name);

    if !tag_path.exists() {
        anyhow::bail!("Tag '{}' not found", name);
    }

    let commit_hash = std::fs::read_to_string(&tag_path)?.trim().to_string();

    // Load the commit
    let commit_path = project
        .ozzy_dir()
        .join("commits")
        .join(format!("{}.json", commit_hash));

    println!("Tag: {}", name);
    println!("Commit: {}", commit_hash);

    if commit_path.exists() {
        let content = std::fs::read_to_string(&commit_path)?;
        let commit: serde_json::Value = serde_json::from_str(&content)?;

        if let Some(msg) = commit["message"].as_str() {
            println!("Message: {}", msg);
        }
        if let Some(author) = commit["author"].as_str() {
            println!("Author: {}", author);
        }
        if let Some(timestamp) = commit["timestamp"].as_str() {
            println!("Date: {}", timestamp);
        }
    }

    Ok(())
}
