//! Commit history API endpoints.
//!
//! `GET /v1/commits/{owner}/{slug}` — list commits for a project
//! `GET /v1/commits/{owner}/{slug}/{sha}` — get commit detail + published project revision

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
use crate::registry::load_published_project_revision_by_commit;

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
    project_meta: serde_json::Value,
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

    let limit = query.limit.max(1).min(100);
    let commits = state.db.list_commits(project.id, limit).await?;

    let mut summaries = Vec::with_capacity(commits.len());
    for c in commits {
        let username = state
            .db
            .get_user_by_id(c.pushed_by)
            .await?
            .map(|u| u.username)
            .unwrap_or_else(|| c.pushed_by.to_string());
        summaries.push(CommitSummary {
            id: c.id.to_string(),
            git_commit_sha: c.git_commit_sha,
            git_provider: c.git_provider,
            git_repo: c.git_repo,
            message: c.message,
            pushed_by: username,
            created_at: c.created_at,
        });
    }

    Ok(Json(summaries))
}

/// Get a single commit by SHA, including its published project revision payloads.
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

    let published =
        load_published_project_revision_by_commit(&state.db, &state.registry_snapshots, commit.id)
            .await
            .map_err(|e| ApiError::Internal(e.into()))?;

    let username = state
        .db
        .get_user_by_id(commit.pushed_by)
        .await?
        .map(|u| u.username)
        .unwrap_or_else(|| commit.pushed_by.to_string());

    Ok(Json(CommitDetail {
        id: commit.id.to_string(),
        git_commit_sha: commit.git_commit_sha,
        git_provider: commit.git_provider,
        git_repo: commit.git_repo,
        ozzy_toml_hash: commit.ozzy_toml_hash,
        message: commit.message,
        pushed_by: username,
        created_at: commit.created_at,
        environments: published.row.environments.clone(),
        transforms: published.row.transforms.clone(),
        endpoints: published.row.endpoints.clone(),
        project_meta: published.project_meta,
    }))
}
