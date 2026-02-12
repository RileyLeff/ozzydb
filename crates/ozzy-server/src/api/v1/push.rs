//! Push endpoint — registers a git commit with the OzzyDB registry.
//!
//! `POST /v1/push` receives a git commit reference, fetches and validates
//! the `ozzy.toml` from the repository, caches the source tarball, and
//! stores the parsed commit state.

use axum::{Json, Router, extract::State, routing::post};
use serde::{Deserialize, Serialize};

use super::access::enforce_write_access;
use super::auth::ApiError;
use crate::AppState;
use crate::auth::middleware::AuthUser;

/// Build the push router.
pub fn router() -> Router<AppState> {
    Router::new().route("/", post(push))
}

/// Push request body.
#[derive(Debug, Deserialize)]
struct PushRequest {
    /// Project identifier: "owner/slug"
    project: String,
    /// Git provider name (e.g., "github")
    git_provider: String,
    /// Git repository (e.g., "rileyleff/sapflux-analysis")
    git_repo: String,
    /// Full git commit SHA
    git_commit_sha: String,
    /// Optional: update this ref (branch name)
    #[serde(rename = "ref")]
    ref_name: Option<String>,
    /// Optional: commit message
    message: Option<String>,
}

/// Push response body.
#[derive(Debug, Serialize)]
struct PushResponse {
    commit_id: String,
    git_commit_sha: String,
    environments: Vec<EnvironmentStatus>,
    source_cached: bool,
}

#[derive(Debug, Serialize)]
struct EnvironmentStatus {
    name: String,
    status: String,
}

/// Validate a git commit SHA (40 lowercase hex chars).
fn is_valid_sha(sha: &str) -> bool {
    sha.len() == 40 && sha.chars().all(|c| c.is_ascii_hexdigit())
}

/// Register a git commit with the OzzyDB registry.
///
/// 1. Verify write access (or create project if first push)
/// 2. Fetch ozzy.toml from git provider at the commit SHA
/// 3. Parse and validate ozzy.toml
/// 4. Verify referenced source files exist at the commit
/// 5. Cache source tarball
/// 6. Insert commit + commit_state records
/// 7. Upsert ref if specified
async fn push(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<PushRequest>,
) -> Result<Json<PushResponse>, ApiError> {
    // ── Validate inputs ──────────────────────────────────────────
    let (owner, slug) = req.project.split_once('/').ok_or_else(|| {
        ApiError::BadRequest("'project' must be in 'owner/slug' format".to_string())
    })?;

    if !is_valid_sha(&req.git_commit_sha) {
        return Err(ApiError::BadRequest(
            "git_commit_sha must be a 40-character hex string".to_string(),
        ));
    }

    if req.git_provider != "github" {
        return Err(ApiError::BadRequest(format!(
            "Unsupported git provider: '{}'. Currently only 'github' is supported.",
            req.git_provider
        )));
    }

    // Validate ref name if provided
    if let Some(ref ref_name) = req.ref_name {
        if ref_name.is_empty() || ref_name.contains("..") || ref_name.starts_with('/') {
            return Err(ApiError::BadRequest("Invalid ref name".to_string()));
        }
    }

    // Verify the pusher's username matches the project owner
    if auth.user.username != owner {
        // Check if user has write access to an existing project
        if let Some(project) = state.db.get_project(owner, slug).await? {
            enforce_write_access(&state, &project, owner, slug, &auth.user, &auth.scope).await?;
        } else {
            return Err(ApiError::forbidden(
                "You can only create projects under your own username",
            ));
        }
    }

    // ── Get or create project ────────────────────────────────────
    let project = state
        .db
        .get_or_create_project(auth.user.id, slug, "private")
        .await
        .map_err(ApiError::Internal)?;

    // If project already existed and owner is the user, verify write access via scope
    enforce_write_access(&state, &project, owner, slug, &auth.user, &auth.scope).await?;

    // Check for duplicate push (same SHA already registered)
    if let Some(existing) = state
        .db
        .get_commit_by_sha(project.id, &req.git_commit_sha)
        .await?
    {
        // Idempotent: return success with existing commit info
        return Ok(Json(PushResponse {
            commit_id: existing.id.to_string(),
            git_commit_sha: existing.git_commit_sha,
            environments: vec![],
            source_cached: true,
        }));
    }

    // ── Fetch ozzy.toml from git provider ────────────────────────
    let toml_bytes = state
        .git
        .get_file(&req.git_repo, &req.git_commit_sha, "ozzy.toml")
        .await
        .map_err(|e| {
            // Convert git errors to appropriate API errors
            if let Some(git_err) = e.downcast_ref::<crate::git::github::GitError>() {
                match git_err {
                    crate::git::github::GitError::FileNotFound { .. } => ApiError::BadRequest(
                        "ozzy.toml not found in repository at the specified commit".to_string(),
                    ),
                    crate::git::github::GitError::InstallationNotFound(owner) => {
                        ApiError::BadRequest(format!(
                            "Cannot access repository. Install the OzzyDB GitHub App for '{}': \
                             https://github.com/apps/ozzydb/installations/new",
                            owner
                        ))
                    }
                    _ => ApiError::Internal(e),
                }
            } else {
                ApiError::Internal(e)
            }
        })?;

    let toml_str = String::from_utf8(toml_bytes)
        .map_err(|_| ApiError::BadRequest("ozzy.toml is not valid UTF-8".to_string()))?;

    // ── Parse and validate ozzy.toml ─────────────────────────────
    let ozzy_toml = ozzy_core::toml_spec::OzzyToml::parse(&toml_str)
        .map_err(|e| ApiError::BadRequest(format!("Failed to parse ozzy.toml: {}", e)))?;

    let validation_errors = ozzy_toml.validate();
    if !validation_errors.is_empty() {
        let messages: Vec<String> = validation_errors
            .iter()
            .map(|e| format!("[{}] {}", e.location, e.message))
            .collect();
        return Err(ApiError::BadRequest(format!(
            "ozzy.toml validation failed:\n{}",
            messages.join("\n")
        )));
    }

    // ── Verify referenced source files exist ─────────────────────
    for (name, transform) in &ozzy_toml.transforms {
        if let Some(source) = &transform.source {
            // Try to fetch the file to verify it exists
            state
                .git
                .get_file(&req.git_repo, &req.git_commit_sha, source)
                .await
                .map_err(|e| {
                    if let Some(crate::git::github::GitError::FileNotFound { .. }) =
                        e.downcast_ref::<crate::git::github::GitError>()
                    {
                        ApiError::BadRequest(format!(
                            "Transform '{}' references source file '{}' which does not exist at commit {}",
                            name,
                            source,
                            req.git_commit_sha.get(..8).unwrap_or(&req.git_commit_sha)
                        ))
                    } else {
                        ApiError::Internal(e)
                    }
                })?;
        }
    }

    // ── Cache source tarball ─────────────────────────────────────
    let source_cached = cache_source_tarball(
        &state,
        &req.git_provider,
        &req.git_repo,
        &req.git_commit_sha,
    )
    .await;
    if let Err(ref e) = source_cached {
        tracing::warn!("Failed to cache source tarball: {}", e);
    }

    // ── Compute ozzy.toml hash and store commit ──────────────────
    let toml_hash = ozzy_core::hash::blake3_hash(toml_str.as_bytes());

    let environments_json =
        serde_json::to_value(&ozzy_toml.environments).map_err(|e| ApiError::Internal(e.into()))?;
    let transforms_json =
        serde_json::to_value(&ozzy_toml.transforms).map_err(|e| ApiError::Internal(e.into()))?;
    let endpoints_json =
        serde_json::to_value(&ozzy_toml.endpoints).map_err(|e| ApiError::Internal(e.into()))?;
    let project_meta_json =
        serde_json::to_value(&ozzy_toml.project).map_err(|e| ApiError::Internal(e.into()))?;

    let commit = state
        .db
        .insert_commit(
            project.id,
            &req.git_provider,
            &req.git_repo,
            &req.git_commit_sha,
            &toml_hash,
            auth.user.id,
            req.message.as_deref(),
        )
        .await
        .map_err(ApiError::Internal)?;

    state
        .db
        .insert_commit_state(
            commit.id,
            &toml_str,
            &environments_json,
            &transforms_json,
            &endpoints_json,
            &project_meta_json,
        )
        .await
        .map_err(ApiError::Internal)?;

    // ── Upsert ref if specified ──────────────────────────────────
    if let Some(ref ref_name) = req.ref_name {
        state
            .db
            .upsert_ref(project.id, ref_name, "branch", commit.id)
            .await
            .map_err(ApiError::Internal)?;
    }

    // ── Build environment status list ────────────────────────────
    // In Phase 3, environment builds are deferred to Phase 4.
    // Report all environments as "pending".
    let env_statuses: Vec<EnvironmentStatus> = ozzy_toml
        .environments
        .keys()
        .map(|name| EnvironmentStatus {
            name: name.clone(),
            status: "pending".to_string(),
        })
        .collect();

    tracing::info!(
        "Push registered: {}/{} at {} (commit_id={})",
        owner,
        slug,
        req.git_commit_sha.get(..8).unwrap_or(&req.git_commit_sha),
        commit.id
    );

    Ok(Json(PushResponse {
        commit_id: commit.id.to_string(),
        git_commit_sha: req.git_commit_sha,
        environments: env_statuses,
        source_cached: source_cached.is_ok(),
    }))
}

/// Fetch and cache the source tarball for a commit.
///
/// Source tarballs are stored with the key pattern `source/{sha}`.
/// This is idempotent — if the tarball is already cached, returns early.
async fn cache_source_tarball(
    state: &AppState,
    git_provider: &str,
    git_repo: &str,
    git_commit_sha: &str,
) -> Result<(), anyhow::Error> {
    // Check if already cached
    if state
        .db
        .get_source_cache(git_provider, git_repo, git_commit_sha)
        .await?
        .is_some()
    {
        state
            .db
            .touch_source_cache(git_provider, git_repo, git_commit_sha)
            .await?;
        return Ok(());
    }

    // Fetch tarball from git provider
    let tarball = state.git.fetch_archive(git_repo, git_commit_sha).await?;
    let byte_size = tarball.len() as i64;

    // Store in source storage using the commit SHA as key
    let source_storage =
        crate::storage::ContentStorage::from_config_with_prefix(&state.config, "source")?;
    source_storage
        .store_with_hash(git_commit_sha, &tarball, "tar.gz")
        .await?;

    // Record in source_cache table
    let r2_key = format!("source/{}.tar.gz", git_commit_sha);
    state
        .db
        .insert_source_cache(git_provider, git_repo, git_commit_sha, &r2_key, byte_size)
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_valid_sha() {
        assert!(is_valid_sha("a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2"));
        assert!(is_valid_sha("0000000000000000000000000000000000000000"));
        assert!(!is_valid_sha("short"));
        assert!(!is_valid_sha("a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2xx")); // too long
        assert!(!is_valid_sha("g1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2")); // invalid hex
        assert!(!is_valid_sha("")); // empty
    }
}
