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

/// Validate a name (alphanumeric, underscores, dashes only).
fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
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

    // Validate owner and slug: alphanumeric, underscores, dashes only
    if !is_valid_name(owner) || !is_valid_name(slug) {
        return Err(ApiError::BadRequest(
            "Owner and slug must be non-empty and contain only alphanumeric characters, underscores, or dashes".to_string(),
        ));
    }

    if !is_valid_sha(&req.git_commit_sha) {
        return Err(ApiError::BadRequest(
            "git_commit_sha must be a 40-character hex string".to_string(),
        ));
    }
    // Normalize to lowercase to avoid case-mismatch in storage keys
    let git_commit_sha = req.git_commit_sha.to_ascii_lowercase();

    if req.git_provider != "github" {
        return Err(ApiError::BadRequest(format!(
            "Unsupported git provider: '{}'. Currently only 'github' is supported.",
            req.git_provider
        )));
    }

    // Validate ref name if provided
    if let Some(ref ref_name) = req.ref_name {
        if ref_name.is_empty()
            || ref_name.contains("..")
            || ref_name.starts_with('/')
            || !ref_name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '/')
        {
            return Err(ApiError::BadRequest("Invalid ref name".to_string()));
        }
    }

    // Validate commit message length
    if let Some(ref msg) = req.message {
        if msg.len() > 10_000 {
            return Err(ApiError::BadRequest(
                "Commit message too long (max 10,000 characters)".to_string(),
            ));
        }
    }

    // ── Resolve project owner ────────────────────────────────────
    let owner_user = state
        .db
        .get_user_by_username(owner)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("User '{}' not found", owner)))?;

    // ── Auth: verify access before any side effects ──────────────
    // Check existing project first to avoid creating projects as a side effect
    // of unauthorized push attempts.
    let existing_project = state.db.get_project(owner, slug).await?;

    if auth.user.username != owner {
        // Non-owner: must be a collaborator on an existing project
        if let Some(ref project) = existing_project {
            enforce_write_access(&state, project, owner, slug, &auth.user, &auth.scope).await?;
        } else {
            return Err(ApiError::forbidden(
                "You can only create projects under your own username",
            ));
        }
    } else if let Some(ref project) = existing_project {
        // Owner, existing project: verify token scope
        enforce_write_access(&state, project, owner, slug, &auth.user, &auth.scope).await?;
    } else {
        // Owner, new project: creating requires account scope (not a project-scoped token)
        if auth.scope != "account" {
            return Err(ApiError::forbidden(
                "Creating new projects requires an account-scoped token",
            ));
        }
    }

    // ── Get or create project (safe — access verified above) ─────
    let project = state
        .db
        .get_or_create_project(owner_user.id, slug, "private")
        .await
        .map_err(ApiError::Internal)?;

    // Check for duplicate push (same SHA already registered)
    if let Some(existing) = state
        .db
        .get_commit_by_sha(project.id, &git_commit_sha)
        .await?
    {
        // Still update the ref if a new one was specified
        if let Some(ref ref_name) = req.ref_name {
            state
                .db
                .upsert_ref(project.id, ref_name, "branch", existing.id)
                .await
                .map_err(ApiError::Internal)?;
        }

        // Check whether source was actually cached on the original push
        let source_cached = state
            .db
            .get_source_cache(&req.git_provider, &req.git_repo, &git_commit_sha)
            .await?
            .is_some();

        // Idempotent: return success with existing commit info
        return Ok(Json(PushResponse {
            commit_id: existing.id.to_string(),
            git_commit_sha: existing.git_commit_sha,
            environments: vec![],
            source_cached,
        }));
    }

    // ── Fetch ozzy.toml from git provider ────────────────────────
    let toml_bytes = state
        .git
        .get_file(&req.git_repo, &git_commit_sha, "ozzy.toml")
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
            // Validate source ref format (file_path:function_name) and character safety
            crate::runners::validate_source_ref(source).map_err(|e| {
                ApiError::BadRequest(format!(
                    "Transform '{}' has invalid source '{}': {}",
                    name, source, e
                ))
            })?;

            // Strip function selector (e.g. "transforms/qc.py:quality_control" → "transforms/qc.py")
            let file_path = source
                .rsplit_once(':')
                .map_or(source.as_str(), |(path, _)| path);

            // Try to fetch the file to verify it exists
            state
                .git
                .get_file(&req.git_repo, &git_commit_sha, file_path)
                .await
                .map_err(|e| {
                    if let Some(crate::git::github::GitError::FileNotFound { .. }) =
                        e.downcast_ref::<crate::git::github::GitError>()
                    {
                        ApiError::BadRequest(format!(
                            "Transform '{}' references source file '{}' which does not exist at commit {}",
                            name,
                            source,
                            git_commit_sha.get(..8).unwrap_or(&git_commit_sha)
                        ))
                    } else {
                        ApiError::Internal(e)
                    }
                })?;
        }
    }

    // ── Cache source tarball ─────────────────────────────────────
    // Source caching is required if any transforms use source files (not command-based).
    let has_source_transforms = ozzy_toml.transforms.values().any(|t| t.source.is_some());
    let source_cached =
        cache_source_tarball(&state, &req.git_provider, &req.git_repo, &git_commit_sha).await;
    if let Err(ref e) = source_cached {
        if has_source_transforms {
            return Err(ApiError::Internal(anyhow::anyhow!(
                "Failed to cache source tarball (required for source-based transforms): {}",
                e
            )));
        }
        tracing::warn!("Failed to cache source tarball (no source transforms, continuing): {}", e);
    }

    // ── Compute ozzy.toml hash and store commit atomically ───────
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
        .register_commit_atomically(
            project.id,
            &req.git_provider,
            &req.git_repo,
            &git_commit_sha,
            &toml_hash,
            auth.user.id,
            req.message.as_deref(),
            &toml_str,
            &environments_json,
            &transforms_json,
            &endpoints_json,
            &project_meta_json,
            req.ref_name.as_deref(),
        )
        .await
        .map_err(ApiError::Internal)?;

    // ── Spawn environment builds ───────────────────────────────
    // Environment builds run asynchronously — don't block the push response.
    // Report initial status as "building" (or "disabled" if compute is off).
    let mut env_statuses = Vec::new();

    if state.config.compute.enabled {
        for (env_name, env_def) in &ozzy_toml.environments {
            let status = match env_def.tier() {
                Some(ozzy_core::toml_spec::EnvironmentTier::Prebuilt { .. }) => "ready".to_string(),
                Some(_) => "building".to_string(),
                None => "invalid".to_string(),
            };
            env_statuses.push(EnvironmentStatus {
                name: env_name.clone(),
                status,
            });
        }

        // Spawn async build tasks for each environment
        let build_state = state.clone();
        let build_envs = ozzy_toml.environments.clone();
        let build_git_repo = req.git_repo.clone();
        let build_git_sha = git_commit_sha.clone();
        tokio::spawn(async move {
            build_environments_async(&build_state, &build_envs, &build_git_repo, &build_git_sha)
                .await;
        });
    } else {
        for env_name in ozzy_toml.environments.keys() {
            env_statuses.push(EnvironmentStatus {
                name: env_name.clone(),
                status: "disabled".to_string(),
            });
        }
    }

    tracing::info!(
        "Push registered: {}/{} at {} (commit_id={})",
        owner,
        slug,
        git_commit_sha.get(..8).unwrap_or(&git_commit_sha),
        commit.id
    );

    Ok(Json(PushResponse {
        commit_id: commit.id.to_string(),
        git_commit_sha,
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

    // Record in source_cache table (use the actual sharded key from storage)
    let r2_key = source_storage.storage_key(git_commit_sha, "tar.gz")?;
    state
        .db
        .insert_source_cache(git_provider, git_repo, git_commit_sha, &r2_key, byte_size)
        .await?;

    Ok(())
}

/// Asynchronously build all environments declared in an `ozzy.toml`.
///
/// Fetches lockfile/Dockerfile content from git as needed, then delegates
/// to the Docker builder. Errors are logged but don't propagate — the push
/// has already succeeded.
async fn build_environments_async(
    state: &AppState,
    environments: &std::collections::HashMap<String, ozzy_core::toml_spec::EnvironmentDef>,
    git_repo: &str,
    git_commit_sha: &str,
) {
    use crate::environments::docker::build_environment;
    use crate::environments::hash::EnvironmentContent;

    for (env_name, env_def) in environments {
        let tier = match env_def.tier() {
            Some(t) => t,
            None => {
                tracing::warn!("Environment '{}' has invalid tier configuration", env_name);
                continue;
            }
        };

        // Fetch content from git as needed
        let content = match &tier {
            ozzy_core::toml_spec::EnvironmentTier::BaseLockfile { lockfile, .. } => {
                match state.git.get_file(git_repo, git_commit_sha, lockfile).await {
                    Ok(bytes) => EnvironmentContent {
                        lockfile_content: Some(String::from_utf8_lossy(&bytes).to_string()),
                        ..Default::default()
                    },
                    Err(e) => {
                        tracing::error!(
                            "Failed to fetch lockfile '{}' for env '{}': {}",
                            lockfile,
                            env_name,
                            e
                        );
                        continue;
                    }
                }
            }
            ozzy_core::toml_spec::EnvironmentTier::Dockerfile { dockerfile } => {
                match state
                    .git
                    .get_file(git_repo, git_commit_sha, dockerfile)
                    .await
                {
                    Ok(bytes) => EnvironmentContent {
                        dockerfile_content: Some(String::from_utf8_lossy(&bytes).to_string()),
                        ..Default::default()
                    },
                    Err(e) => {
                        tracing::error!(
                            "Failed to fetch Dockerfile '{}' for env '{}': {}",
                            dockerfile,
                            env_name,
                            e
                        );
                        continue;
                    }
                }
            }
            ozzy_core::toml_spec::EnvironmentTier::Prebuilt { .. } => EnvironmentContent::default(),
        };

        // Pre-compute env hash so we can clean up on failure
        let env_hash =
            crate::environments::hash::compute_env_hash(&tier, &content);

        match build_environment(&state.db, &state.config.compute, env_name, &tier, &content).await {
            Ok(result) => {
                tracing::info!(
                    "Environment '{}' ready: {} (type={}, {}ms)",
                    env_name,
                    result.image_ref,
                    result.build_type,
                    result.build_duration_ms.unwrap_or(0)
                );
            }
            Err(e) => {
                tracing::error!("Failed to build environment '{}': {}", env_name, e);
                // Delete the pending row so a subsequent push can retry the build
                if let Err(del_err) = state.db.delete_pending_environment_image(&env_hash).await {
                    tracing::error!(
                        "Failed to clean up pending env '{}' after build failure: {}",
                        env_name,
                        del_err
                    );
                }
            }
        }
    }
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

    #[test]
    fn test_is_valid_name() {
        assert!(is_valid_name("my-project"));
        assert!(is_valid_name("my_project"));
        assert!(is_valid_name("project123"));
        assert!(!is_valid_name("")); // empty
        assert!(!is_valid_name("my/project")); // slash
        assert!(!is_valid_name("my project")); // space
        assert!(!is_valid_name("my.project")); // dot
    }
}
