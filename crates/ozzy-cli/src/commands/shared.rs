//! Shared helpers for CLI commands.

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use super::auth::{self, Credentials};

/// API error response shape.
#[derive(Debug, Deserialize)]
pub struct ErrorResponse {
    #[allow(dead_code)]
    pub error: Option<String>,
    pub message: Option<String>,
}

/// Load credentials or bail with a helpful message.
pub fn require_auth() -> Result<Credentials> {
    auth::load_credentials()?
        .ok_or_else(|| anyhow::anyhow!("Not logged in. Run `ozzy auth login` first."))
}

/// Get the registry URL from credentials or fall back to default.
pub fn registry_url(creds: &Credentials) -> &str {
    &creds.registry_url
}

/// Build an HTTP client that doesn't follow redirects (for presigned URL handling).
pub fn http_client_no_redirect() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .context("Failed to create HTTP client")
}

/// Build a standard HTTP client.
pub fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .build()
        .context("Failed to create HTTP client")
}

/// Parse a project reference from ozzy.toml in the current directory.
/// Returns (owner, slug, git_provider, git_repo).
pub fn load_project_from_toml() -> Result<ProjectRef> {
    let toml_str =
        std::fs::read_to_string("ozzy.toml").context("No ozzy.toml found in current directory")?;
    let toml: toml::Table = toml_str.parse().context("Failed to parse ozzy.toml")?;

    let project = toml
        .get("project")
        .and_then(|p| p.as_table())
        .ok_or_else(|| anyhow::anyhow!("Missing [project] in ozzy.toml"))?;

    let name = project
        .get("name")
        .and_then(|n| n.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing project.name in ozzy.toml"))?;

    let owner = project
        .get("owner")
        .and_then(|o| o.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing project.owner in ozzy.toml"))?;

    let git = toml
        .get("git")
        .and_then(|g| g.as_table())
        .ok_or_else(|| anyhow::anyhow!("Missing [git] in ozzy.toml"))?;

    let provider = git
        .get("provider")
        .and_then(|p| p.as_str())
        .unwrap_or("github");

    let repo = git
        .get("repo")
        .and_then(|r| r.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing git.repo in ozzy.toml"))?;

    Ok(ProjectRef {
        owner: owner.to_string(),
        slug: name.to_string(),
        git_provider: provider.to_string(),
        git_repo: repo.to_string(),
    })
}

/// Parsed project reference from ozzy.toml.
#[derive(Debug, Clone)]
pub struct ProjectRef {
    pub owner: String,
    pub slug: String,
    pub git_provider: String,
    pub git_repo: String,
}

impl ProjectRef {
    /// "owner/slug" format.
    pub fn project_path(&self) -> String {
        format!("{}/{}", self.owner, self.slug)
    }
}

/// Validate that a name is safe for use in URL path segments.
/// Must match `[a-zA-Z0-9_-]+` (server enforces the same pattern).
pub fn validate_name(name: &str, kind: &str) -> Result<()> {
    if name.is_empty() {
        bail!("{} name cannot be empty", kind);
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        bail!(
            "Invalid {} name '{}': must contain only letters, digits, underscores, and hyphens",
            kind,
            name
        );
    }
    Ok(())
}

/// Extract error message from a non-success response.
pub async fn extract_error(resp: reqwest::Response) -> String {
    let status = resp.status();
    match resp.json::<ErrorResponse>().await {
        Ok(e) => format!(
            "({}) {}",
            status,
            e.message
                .or(e.error)
                .unwrap_or_else(|| "Unknown error".to_string())
        ),
        Err(_) => format!("({})", status),
    }
}

/// Get the current git commit SHA (full 40-char hex).
pub fn current_git_sha() -> Result<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .context("Failed to run git rev-parse HEAD")?;

    if !output.status.success() {
        bail!(
            "Not a git repository or no commits: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(String::from_utf8(output.stdout)
        .context("Invalid UTF-8 from git")?
        .trim()
        .to_string())
}

/// Get the current git branch name.
pub fn current_git_branch() -> Result<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .context("Failed to run git rev-parse --abbrev-ref HEAD")?;

    if !output.status.success() {
        bail!("Failed to get current branch");
    }

    Ok(String::from_utf8(output.stdout)
        .context("Invalid UTF-8 from git")?
        .trim()
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_name_accepts_valid() {
        assert!(validate_name("my-dataset_v2", "data atom").is_ok());
        assert!(validate_name("ABC123", "test").is_ok());
    }

    #[test]
    fn test_validate_name_rejects_invalid() {
        assert!(validate_name("", "test").is_err());
        assert!(validate_name("has/slash", "test").is_err());
        assert!(validate_name("has space", "test").is_err());
        assert!(validate_name("has?query", "test").is_err());
        assert!(validate_name("has#hash", "test").is_err());
    }

    #[test]
    fn test_project_ref_project_path() {
        let pr = ProjectRef {
            owner: "alice".to_string(),
            slug: "myproject".to_string(),
            git_provider: "github".to_string(),
            git_repo: "alice/myproject".to_string(),
        };
        assert_eq!(pr.project_path(), "alice/myproject");
    }
}
