//! Commit history API endpoints.
//!
//! `GET /v1/commits/{owner}/{slug}` — list commits for a project
//! `GET /v1/commits/{owner}/{slug}/{sha}` — get commit detail + commit state

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::get,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::access::enforce_read_access;
use super::auth::ApiError;
use crate::AppState;
use crate::auth::middleware::MaybeAuthUser;

/// Build the commits router.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/{owner}/{slug}", get(list_commits))
        .route("/{owner}/{slug}/{sha}", get(get_commit))
}

// ============================================================================
// Wire types
// ============================================================================

#[derive(Debug, Deserialize)]
struct ListQuery {
    #[serde(default = "default_limit")]
    limit: i64,
}

fn default_limit() -> i64 {
    50
}

#[derive(Debug, Serialize)]
struct CommitSummary {
    id: String,
    git_commit_sha: String,
    git_provider: String,
    git_repo: String,
    message: Option<String>,
    pushed_by: String,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct CommitDetail {
    id: String,
    git_commit_sha: String,
    git_provider: String,
    git_repo: String,
    ozzy_toml_hash: String,
    message: Option<String>,
    pushed_by: String,
    created_at: DateTime<Utc>,
    environments: serde_json::Value,
    transforms: serde_json::Value,
    endpoints: serde_json::Value,
}

// ============================================================================
// Handlers
// ============================================================================

/// List commits for a project, most recent first.
async fn list_commits(
    State(state): State<AppState>,
    Path((owner, slug)): Path<(String, String)>,
    Query(query): Query<ListQuery>,
    auth: MaybeAuthUser,
) -> Result<Json<Vec<CommitSummary>>, ApiError> {
    let project =
        state.db.get_project(&owner, &slug).await?.ok_or_else(|| {
            ApiError::not_found(format!("Project '{}/{}' not found", owner, slug))
        })?;

    enforce_read_access(&state, &project, &owner, &slug, &auth).await?;

    let commits = state
        .db
        .list_commits(project.id, query.limit.min(100))
        .await?;

    let summaries: Vec<CommitSummary> = commits
        .into_iter()
        .map(|c| CommitSummary {
            id: c.id.to_string(),
            git_commit_sha: c.git_commit_sha,
            git_provider: c.git_provider,
            git_repo: c.git_repo,
            message: c.message,
            pushed_by: c.pushed_by.to_string(),
            created_at: c.created_at,
        })
        .collect();

    Ok(Json(summaries))
}

/// Get a single commit by SHA, including its commit state.
async fn get_commit(
    State(state): State<AppState>,
    Path((owner, slug, sha)): Path<(String, String, String)>,
    auth: MaybeAuthUser,
) -> Result<Json<CommitDetail>, ApiError> {
    let project =
        state.db.get_project(&owner, &slug).await?.ok_or_else(|| {
            ApiError::not_found(format!("Project '{}/{}' not found", owner, slug))
        })?;

    enforce_read_access(&state, &project, &owner, &slug, &auth).await?;

    let commit = state
        .db
        .get_commit_by_sha(project.id, &sha)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Commit '{}' not found", sha)))?;

    let commit_state = state.db.get_commit_state(commit.id).await?;

    let (environments, transforms, endpoints) = match commit_state {
        Some(cs) => (cs.environments, cs.transforms, cs.endpoints),
        None => (
            serde_json::json!({}),
            serde_json::json!({}),
            serde_json::json!({}),
        ),
    };

    Ok(Json(CommitDetail {
        id: commit.id.to_string(),
        git_commit_sha: commit.git_commit_sha,
        git_provider: commit.git_provider,
        git_repo: commit.git_repo,
        ozzy_toml_hash: commit.ozzy_toml_hash,
        message: commit.message,
        pushed_by: commit.pushed_by.to_string(),
        created_at: commit.created_at,
        environments,
        transforms,
        endpoints,
    }))
}
