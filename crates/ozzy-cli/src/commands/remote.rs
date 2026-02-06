//! Remote registry management commands.

use anyhow::Result;
use ozzy_core::{Project, validate_safe_name};

/// Add a remote registry to the current project.
pub async fn add(name: &str, url: &str) -> Result<()> {
    validate_safe_name(name)?;

    // Validate URL format (must be http:// or https:// with a host)
    if !url.starts_with("http://") && !url.starts_with("https://") {
        anyhow::bail!("Invalid remote URL: must start with http:// or https://");
    }
    let after_scheme = url.split("://").nth(1).unwrap_or("");
    let host_part = after_scheme.split('/').next().unwrap_or("");
    if host_part.is_empty() {
        anyhow::bail!("Invalid remote URL: must include a host");
    }

    let mut project = Project::find_current()?;
    if project.config.remotes.contains_key(name) {
        anyhow::bail!(
            "Remote '{}' already exists. Use 'ozzy remote rm {}' first.",
            name,
            name
        );
    }
    project
        .config
        .remotes
        .insert(name.to_string(), url.to_string());
    project.save_config()?;

    println!("Added remote '{}' -> {}", name, url);

    Ok(())
}

/// Remove a remote registry from the current project.
pub async fn remove(name: &str) -> Result<()> {
    validate_safe_name(name)?;
    let mut project = Project::find_current()?;

    if project.config.remotes.remove(name).is_none() {
        anyhow::bail!("Remote '{}' not found", name);
    }
    project.save_config()?;

    println!("Removed remote '{}'", name);

    Ok(())
}

/// List configured remotes for the current project.
pub async fn list() -> Result<()> {
    let project = Project::find_current()?;

    if !project.config.remotes.is_empty() {
        println!("Configured remotes:");
        for (name, url) in &project.config.remotes {
            println!("  {} -> {}", name, url);
        }
    } else {
        println!("No remotes configured.");
        println!();
        println!("Add a remote with: ozzy remote add <name> <url>");
    }

    Ok(())
}

/// Get the URL for a named remote, or the default remote URL.
pub fn get_remote_url(project: &Project, remote_name: Option<&str>) -> Result<(String, String)> {
    // If a specific remote is requested
    if let Some(name) = remote_name {
        let url = project
            .config
            .remotes
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("Remote '{}' not found", name))?;
        return Ok((name.to_string(), url.clone()));
    }

    // Otherwise, try to find "origin" or the only remote
    if let Some(url) = project.config.remotes.get("origin") {
        return Ok(("origin".to_string(), url.clone()));
    }
    if project.config.remotes.len() == 1 {
        let (name, url) = project.config.remotes.iter().next().expect("len == 1");
        return Ok((name.clone(), url.clone()));
    }

    anyhow::bail!("No remote configured. Use 'ozzy remote add <name> <url>' to add one.")
}
