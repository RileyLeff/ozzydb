//! Remote registry management commands.

use anyhow::{Context, Result};
use ozzy_core::{validate_safe_name, Project};

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

    let project = Project::find_current()?;

    // Load current config
    let config_path = project.root.join("ozzy.toml");
    let content = std::fs::read_to_string(&config_path)?;
    let mut config: toml::Value = toml::from_str(&content)?;

    // Get or create remotes table
    let remotes = config
        .as_table_mut()
        .context("Invalid TOML configuration")?
        .entry("remotes")
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
        .as_table_mut()
        .context("Invalid remotes configuration")?;

    if remotes.contains_key(name) {
        anyhow::bail!("Remote '{}' already exists. Use 'ozzy remote rm {}' first.", name, name);
    }

    remotes.insert(name.to_string(), toml::Value::String(url.to_string()));

    // Save config
    let new_content = toml::to_string_pretty(&config)?;
    std::fs::write(&config_path, new_content)?;

    println!("Added remote '{}' -> {}", name, url);

    Ok(())
}

/// Remove a remote registry from the current project.
pub async fn remove(name: &str) -> Result<()> {
    validate_safe_name(name)?;
    let project = Project::find_current()?;

    // Load current config
    let config_path = project.root.join("ozzy.toml");
    let content = std::fs::read_to_string(&config_path)?;
    let mut config: toml::Value = toml::from_str(&content)?;

    // Get remotes table
    let remotes = config
        .as_table_mut()
        .context("Invalid TOML configuration")?
        .get_mut("remotes")
        .and_then(|v| v.as_table_mut())
        .context("No remotes configured")?;

    if remotes.remove(name).is_none() {
        anyhow::bail!("Remote '{}' not found", name);
    }

    // Save config
    let new_content = toml::to_string_pretty(&config)?;
    std::fs::write(&config_path, new_content)?;

    println!("Removed remote '{}'", name);

    Ok(())
}

/// List configured remotes for the current project.
pub async fn list() -> Result<()> {
    let project = Project::find_current()?;

    // Load current config
    let config_path = project.root.join("ozzy.toml");
    let content = std::fs::read_to_string(&config_path)?;
    let config: toml::Value = toml::from_str(&content)?;

    // Get remotes table
    let remotes = config
        .get("remotes")
        .and_then(|v| v.as_table());

    match remotes {
        Some(remotes) if !remotes.is_empty() => {
            println!("Configured remotes:");
            for (name, url) in remotes {
                if let Some(url_str) = url.as_str() {
                    println!("  {} -> {}", name, url_str);
                }
            }
        }
        _ => {
            println!("No remotes configured.");
            println!();
            println!("Add a remote with: ozzy remote add <name> <url>");
        }
    }

    Ok(())
}

/// Get the URL for a named remote, or the default remote URL.
pub fn get_remote_url(project: &Project, remote_name: Option<&str>) -> Result<(String, String)> {
    let config_path = project.root.join("ozzy.toml");
    let content = std::fs::read_to_string(&config_path)?;
    let config: toml::Value = toml::from_str(&content)?;

    let remotes = config
        .get("remotes")
        .and_then(|v| v.as_table());

    // If a specific remote is requested
    if let Some(name) = remote_name {
        let url = remotes
            .and_then(|r| r.get(name))
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Remote '{}' not found", name))?;
        return Ok((name.to_string(), url.to_string()));
    }

    // Otherwise, try to find "origin" or the only remote
    if let Some(remotes) = remotes {
        if let Some(url) = remotes.get("origin").and_then(|v| v.as_str()) {
            return Ok(("origin".to_string(), url.to_string()));
        }
        if remotes.len() == 1 {
            let (name, url) = remotes.iter().next().unwrap();
            if let Some(url_str) = url.as_str() {
                return Ok((name.clone(), url_str.to_string()));
            }
        }
    }

    anyhow::bail!("No remote configured. Use 'ozzy remote add <name> <url>' to add one.")
}
