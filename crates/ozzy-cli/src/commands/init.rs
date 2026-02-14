//! `ozzy init` — initialize a new OzzyDB project.
//!
//! Detects git repo, detects language/runtime, generates ozzy.toml.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

/// Run the init command in the current directory.
pub fn run(cwd: &Path) -> Result<()> {
    let toml_path = cwd.join("ozzy.toml");

    // Already initialized?
    if toml_path.exists() {
        println!("Project already initialized (ozzy.toml exists).");
        return Ok(());
    }

    // Detect git repo
    let git_info = detect_git(cwd);
    if let Some((provider, repo)) = &git_info {
        println!("Detected git repo: {}.com/{}", provider, repo);
    } else {
        eprintln!("Warning: Not inside a git repository. Git integration will be unavailable.");
        eprintln!("  Run `git init` first for full functionality.");
    }

    // Detect language/runtime
    let env = detect_environment(cwd);
    if let Some((ref desc, _, _)) = env {
        println!("Detected: {}", desc);
    }

    // Infer project name and owner
    let project_name = if let Some((_, ref repo)) = git_info {
        // Use repo name from git remote (part after /)
        repo.split('/').last().unwrap_or("my-project").to_string()
    } else {
        cwd.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("my-project")
            .to_string()
    };

    let owner = if let Some((_, ref repo)) = git_info {
        // Use owner from git remote (part before /)
        repo.split('/').next().unwrap_or("owner").to_string()
    } else {
        "owner".to_string()
    };

    // Build ozzy.toml content
    let mut toml = String::new();

    toml.push_str("[project]\n");
    toml.push_str(&format!("name = \"{}\"\n", project_name));
    toml.push_str(&format!("owner = \"{}\"\n", owner));
    toml.push('\n');

    if let Some((provider, repo)) = &git_info {
        toml.push_str("[git]\n");
        toml.push_str(&format!("provider = \"{}\"\n", provider));
        toml.push_str(&format!("repo = \"{}\"\n", repo));
        toml.push('\n');

        if provider != "github" {
            println!(
                "Warning: provider '{}' detected, but only 'github' is currently supported for push/fetch.",
                provider
            );
        }
    }

    toml.push_str("[remote]\n");
    toml.push_str("url = \"https://api.ozzydb.com\"\n");
    toml.push('\n');

    // Environment section
    if let Some((_, base, lockfile)) = &env {
        toml.push_str("[environments.default]\n");
        toml.push_str(&format!("base = \"{}\"\n", base));
        toml.push_str(&format!("lockfile = \"{}\"\n", lockfile));
    } else {
        toml.push_str("# [environments.default]\n");
        toml.push_str("# base = \"ozzydb/python:3.12\"\n");
        toml.push_str("# lockfile = \"uv.lock\"\n");
    }
    toml.push('\n');

    // Transform and endpoint placeholders
    toml.push_str("# Add transforms here:\n");
    toml.push_str("# [transforms.my_transform]\n");
    toml.push_str("# source = \"transforms/my_transform.py:my_function\"\n");
    toml.push_str("# environment = \"default\"\n");
    toml.push_str("# inputs.data = \"parquet\"\n");
    toml.push_str("# output = \"parquet\"\n");
    toml.push('\n');
    toml.push_str("# Add endpoints here:\n");
    toml.push_str("# [endpoints.my_endpoint]\n");
    toml.push_str("# description = \"...\"\n");
    toml.push_str("# [endpoints.my_endpoint.nodes]\n");
    toml.push_str("# step = { transform = \"my_transform\" }\n");
    toml.push_str("# edges = [\n");
    toml.push_str("#   { from = \"data:my_data\", to = \"step.data\" },\n");
    toml.push_str("# ]\n");

    std::fs::write(&toml_path, &toml).context("Failed to write ozzy.toml")?;

    // Create transforms directory
    let transforms_dir = cwd.join("transforms");
    if !transforms_dir.exists() {
        std::fs::create_dir_all(&transforms_dir).context("Failed to create transforms/")?;
    }

    // Update .gitignore
    update_gitignore(cwd)?;

    println!("\nCreated ozzy.toml");
    println!();
    println!("Next steps:");
    println!("  1. Upload data:       ozzy data upload <file>");
    println!("  2. Add transforms:    edit ozzy.toml [transforms] section");
    println!("  3. Define endpoints:  edit ozzy.toml [endpoints] section");
    println!("  4. Push to registry:  ozzy push");
    println!("  5. Fetch results:     ozzy fetch <owner/project/endpoint>");

    Ok(())
}

/// Detect git provider and repo from the origin remote.
///
/// Returns `Some(("github", "owner/repo"))` or `None`.
fn detect_git(cwd: &Path) -> Option<(String, String)> {
    let output = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(cwd)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    parse_git_remote(&url)
}

/// Parse a git remote URL into (provider, owner/repo).
fn parse_git_remote(url: &str) -> Option<(String, String)> {
    // SSH: git@github.com:owner/repo.git
    if let Some(rest) = url.strip_prefix("git@") {
        let (host, path) = rest.split_once(':')?;
        let provider = host_to_provider(host)?;
        let repo = path.trim_end_matches(".git");
        return Some((provider, repo.to_string()));
    }

    // HTTPS: https://github.com/owner/repo.git
    if url.starts_with("https://") || url.starts_with("http://") {
        let without_scheme = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))?;
        let (host, path) = without_scheme.split_once('/')?;
        let provider = host_to_provider(host)?;
        let repo = path.trim_end_matches(".git").trim_end_matches('/');
        return Some((provider, repo.to_string()));
    }

    None
}

fn host_to_provider(host: &str) -> Option<String> {
    match host {
        "github.com" => Some("github".to_string()),
        "gitlab.com" => Some("gitlab".to_string()),
        _ => None,
    }
}

/// Detect project language/runtime.
///
/// Returns `Some((description, base_image, lockfile))` or None.
fn detect_environment(cwd: &Path) -> Option<(String, String, String)> {
    // Check Python lockfiles (preferred first)
    if cwd.join("uv.lock").exists() {
        return Some((
            "Python project (uv.lock)".to_string(),
            detect_python_version(cwd),
            "uv.lock".to_string(),
        ));
    }
    if cwd.join("poetry.lock").exists() {
        return Some((
            "Python project (poetry.lock)".to_string(),
            detect_python_version(cwd),
            "poetry.lock".to_string(),
        ));
    }
    if cwd.join("Pipfile.lock").exists() {
        return Some((
            "Python project (Pipfile.lock)".to_string(),
            detect_python_version(cwd),
            "Pipfile.lock".to_string(),
        ));
    }
    if cwd.join("requirements.txt").exists() {
        return Some((
            "Python project (requirements.txt)".to_string(),
            detect_python_version(cwd),
            "requirements.txt".to_string(),
        ));
    }

    // R
    if cwd.join("renv.lock").exists() {
        return Some((
            "R project (renv.lock)".to_string(),
            "ozzydb/r:4.4".to_string(),
            "renv.lock".to_string(),
        ));
    }

    // Julia
    if cwd.join("Manifest.toml").exists() && cwd.join("Project.toml").exists() {
        return Some((
            "Julia project (Manifest.toml)".to_string(),
            "ozzydb/julia:1.11".to_string(),
            "Manifest.toml".to_string(),
        ));
    }

    // pyproject.toml without lockfile
    if cwd.join("pyproject.toml").exists() {
        return Some((
            "Python project (pyproject.toml, no lockfile found)".to_string(),
            detect_python_version(cwd),
            "pyproject.toml".to_string(),
        ));
    }

    None
}

/// Try to detect Python version from pyproject.toml, default to 3.12.
fn detect_python_version(_cwd: &Path) -> String {
    // Could parse pyproject.toml requires-python, but for now default to 3.12
    "ozzydb/python:3.12".to_string()
}

/// Append OzzyDB entries to .gitignore if not already present.
fn update_gitignore(cwd: &Path) -> Result<()> {
    let gitignore_path = cwd.join(".gitignore");
    let existing = if gitignore_path.exists() {
        std::fs::read_to_string(&gitignore_path)?
    } else {
        String::new()
    };

    let mut additions = Vec::new();

    // Lines to ensure are present
    let entries = ["# OzzyDB", ".ozzy/", "data/*.parquet"];

    let existing_lines: Vec<&str> = existing.lines().map(|l| l.trim()).collect();
    for entry in &entries {
        if !existing_lines.contains(entry) {
            additions.push(*entry);
        }
    }

    if !additions.is_empty() {
        let mut content = existing;
        if !content.is_empty() && !content.ends_with('\n') {
            content.push('\n');
        }
        if !content.is_empty() {
            content.push('\n');
        }
        for line in &additions {
            content.push_str(line);
            content.push('\n');
        }
        std::fs::write(&gitignore_path, content)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_git_remote_ssh() {
        let result = parse_git_remote("git@github.com:rileyleff/sapflux-analysis.git");
        assert_eq!(
            result,
            Some((
                "github".to_string(),
                "rileyleff/sapflux-analysis".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_git_remote_https() {
        let result = parse_git_remote("https://github.com/rileyleff/sapflux-analysis.git");
        assert_eq!(
            result,
            Some((
                "github".to_string(),
                "rileyleff/sapflux-analysis".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_git_remote_https_no_git_suffix() {
        let result = parse_git_remote("https://github.com/rileyleff/sapflux-analysis");
        assert_eq!(
            result,
            Some((
                "github".to_string(),
                "rileyleff/sapflux-analysis".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_git_remote_gitlab() {
        let result = parse_git_remote("git@gitlab.com:user/repo.git");
        assert_eq!(
            result,
            Some(("gitlab".to_string(), "user/repo".to_string()))
        );
    }

    #[test]
    fn test_parse_git_remote_unknown_host() {
        let result = parse_git_remote("git@bitbucket.org:user/repo.git");
        assert_eq!(result, None);
    }

    #[test]
    fn test_init_creates_files() {
        let dir = tempfile::tempdir().unwrap();
        run(dir.path()).unwrap();

        assert!(dir.path().join("ozzy.toml").exists());
        assert!(dir.path().join("transforms").exists());
        assert!(dir.path().join(".gitignore").exists());

        let toml = std::fs::read_to_string(dir.path().join("ozzy.toml")).unwrap();
        assert!(toml.contains("[project]"));
        assert!(toml.contains("[remote]"));
        assert!(toml.contains("https://api.ozzydb.com"));
    }

    #[test]
    fn test_init_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        run(dir.path()).unwrap();
        // Second init should succeed (already initialized)
        run(dir.path()).unwrap();

        // ozzy.toml should still exist and be unchanged
        assert!(dir.path().join("ozzy.toml").exists());
    }

    #[test]
    fn test_init_detects_python_uv() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("uv.lock"), "# uv lock").unwrap();
        run(dir.path()).unwrap();

        let toml = std::fs::read_to_string(dir.path().join("ozzy.toml")).unwrap();
        assert!(toml.contains("[environments.default]"));
        assert!(toml.contains("ozzydb/python:3.12"));
        assert!(toml.contains("uv.lock"));
    }

    #[test]
    fn test_init_detects_r_renv() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("renv.lock"), "{}").unwrap();
        run(dir.path()).unwrap();

        let toml = std::fs::read_to_string(dir.path().join("ozzy.toml")).unwrap();
        assert!(toml.contains("[environments.default]"));
        assert!(toml.contains("ozzydb/r:4.4"));
        assert!(toml.contains("renv.lock"));
    }

    #[test]
    fn test_gitignore_appended() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".gitignore"), "*.log\n").unwrap();
        run(dir.path()).unwrap();

        let gitignore = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert!(gitignore.contains("*.log"));
        assert!(gitignore.contains(".ozzy/"));
        assert!(gitignore.contains("data/*.parquet"));
    }
}
