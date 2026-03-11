//! `ozzy push` — register a git commit with the OzzyDB registry.
//!
//! Reads project info from ozzy.toml, resolves the current git SHA,
//! and POSTs to the registry's push endpoint.

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use super::shared;

#[derive(Debug, Serialize)]
struct PushRequest {
    project: String,
    git_repo: String,
    git_commit_sha: String,
    #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
    ref_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct EnvironmentStatus {
    name: String,
    status: String,
}

#[derive(Debug, Deserialize)]
struct PushResponse {
    commit_id: String,
    git_commit_sha: String,
    environments: Vec<EnvironmentStatus>,
    source_cached: bool,
}

/// Execute `ozzy push`.
pub async fn run(ref_name: Option<&str>, message: Option<&str>) -> Result<()> {
    let creds = shared::require_auth()?;
    let project = shared::load_project_from_toml()?;
    let git_sha = shared::current_git_sha()?;

    // Default ref to current branch if not specified
    let ref_name = match ref_name {
        Some(r) => Some(r.to_string()),
        None => Some(shared::current_git_branch()?),
    };

    let registry_url = shared::registry_url(&creds);
    let client = shared::http_client()?;
    let project_path = project.project_path();

    let req = PushRequest {
        project: project_path.clone(),
        git_repo: project.git_repo,
        git_commit_sha: git_sha.clone(),
        ref_name,
        message: message.map(String::from),
    };

    eprintln!(
        "Pushing {} at {}...",
        project_path,
        git_sha.get(..8).unwrap_or(&git_sha)
    );

    let resp = client
        .post(format!("{}/api/v1/push", registry_url))
        .bearer_auth(&creds.token)
        .json(&req)
        .send()
        .await?;

    if !resp.status().is_success() {
        let err = shared::extract_error(resp).await;
        bail!("Push failed: {}", err);
    }

    let push: PushResponse = resp.json().await?;

    println!(
        "Pushed commit {} ({})",
        push.commit_id,
        push.git_commit_sha.get(..8).unwrap_or(&push.git_commit_sha)
    );

    if push.source_cached {
        println!("  Source: cached");
    } else {
        println!("  Source: not cached (no source-based transforms)");
    }

    for env in &push.environments {
        println!("  Environment {}: {}", env.name, env.status);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_request_serialization() {
        let req = PushRequest {
            project: "alice/myproject".to_string(),
            git_repo: "alice/myproject".to_string(),
            git_commit_sha: "a".repeat(40),
            ref_name: Some("main".to_string()),
            message: Some("initial push".to_string()),
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["project"], "alice/myproject");
        assert_eq!(json["ref"], "main");
        assert_eq!(json["message"], "initial push");
    }

    #[test]
    fn test_push_request_skips_none() {
        let req = PushRequest {
            project: "alice/myproject".to_string(),
            git_repo: "alice/myproject".to_string(),
            git_commit_sha: "a".repeat(40),
            ref_name: None,
            message: None,
        };
        let json = serde_json::to_value(&req).unwrap();
        assert!(json.get("ref").is_none());
        assert!(json.get("message").is_none());
    }
}
