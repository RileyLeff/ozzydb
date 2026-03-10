//! Push endpoint — registers a git commit with the OzzyDB registry.
//!
//! `POST /v1/push` receives a git commit reference, fetches and validates
//! the `ozzy.toml` from the repository, caches the source tarball, and
//! publishes a new v4 project revision.

use std::collections::BTreeMap;

use axum::{Json, Router, extract::State, routing::post};
use serde::{Deserialize, Serialize};

use super::access::enforce_write_access;
use super::auth::ApiError;
use crate::AppState;
use crate::auth::middleware::AuthUser;
use crate::db::v4::StoredEnvironmentVersion;
use crate::publication::{PublishCommitInput, PublishOutcome, publish_v4_commit_atomically};
use ozzy_core::toml_spec::{EnvironmentDef, EnvironmentTier, PublishedEnvironmentDef};

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
/// 6. Publish a v4 registry revision + project revision atomically
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
        if state
            .db
            .get_v4_project_revision_by_commit(existing.id)
            .await
            .map_err(|e| ApiError::Internal(e.into()))?
            .is_none()
        {
            return Err(ApiError::Internal(anyhow::anyhow!(
                "commit {} exists without a published v4 project revision",
                existing.id
            )));
        }

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

    let published_environments = resolve_published_environments(
        &state,
        &ozzy_toml.environments,
        &req.git_repo,
        &git_commit_sha,
    )
    .await?;

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
        tracing::warn!(
            "Failed to cache source tarball (no source transforms, continuing): {}",
            e
        );
    }

    // ── Compute ozzy.toml hash and publish atomically ────────────
    let toml_hash = ozzy_core::hash::blake3_hash(toml_str.as_bytes());
    let publish_outcome = publish_v4_commit_atomically(
        &state.db,
        PublishCommitInput {
            project_id: project.id,
            pushed_by: auth.user.id,
            git_provider: &req.git_provider,
            git_repo: &req.git_repo,
            git_commit_sha: &git_commit_sha,
            ozzy_toml_hash: &toml_hash,
            message: req.message.as_deref(),
            ref_name: req.ref_name.as_deref(),
            ozzy_toml_raw: &toml_str,
            ozzy_toml: &ozzy_toml,
            published_environments: &published_environments,
        },
    )
    .await
    .map_err(|e| ApiError::Internal(e.into()))?;

    let (commit, created_new_revision, published_environment_versions) = match publish_outcome {
        PublishOutcome::Created { commit, bundle } => (commit, true, Some(bundle.environments)),
        PublishOutcome::Existing { commit } => (commit, false, None),
    };

    // ── Spawn environment builds ───────────────────────────────
    // Environment builds run asynchronously — don't block the push response.
    // Report initial status as "building" (or "disabled" if compute is off).
    let mut env_statuses = Vec::new();

    if created_new_revision && state.compute.is_enabled() {
        let published_environment_versions = published_environment_versions.ok_or_else(|| {
            ApiError::Internal(anyhow::anyhow!(
                "created publication is missing published environments"
            ))
        })?;
        for (env_name, env_row) in &published_environment_versions {
            let definition: PublishedEnvironmentDef =
                serde_json::from_value(env_row.definition.clone())
                    .map_err(|err| ApiError::Internal(err.into()))?;
            let status = match definition {
                PublishedEnvironmentDef::Prebuilt { .. } => "ready".to_string(),
                _ => "building".to_string(),
            };
            env_statuses.push(EnvironmentStatus {
                name: env_name.clone(),
                status,
            });
        }

        // Spawn async build tasks for each environment
        let build_state = state.clone();
        let build_envs = published_environment_versions;
        tokio::spawn(async move {
            build_environments_async(&build_state, &build_envs).await;
        });
    } else if created_new_revision {
        for env_name in published_environments.keys() {
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

async fn resolve_published_environments(
    state: &AppState,
    environments: &std::collections::HashMap<String, EnvironmentDef>,
    git_repo: &str,
    git_commit_sha: &str,
) -> Result<BTreeMap<String, PublishedEnvironmentDef>, ApiError> {
    let mut published = BTreeMap::new();

    for (env_name, env_def) in environments {
        let tier = env_def.tier().ok_or_else(|| {
            ApiError::BadRequest(format!(
                "Environment '{}' has invalid tier configuration",
                env_name
            ))
        })?;

        let definition = resolve_published_environment_definition(
            state,
            env_name,
            &tier,
            git_repo,
            git_commit_sha,
        )
        .await?;
        published.insert(env_name.clone(), definition);
    }

    Ok(published)
}

async fn resolve_published_environment_definition(
    state: &AppState,
    env_name: &str,
    tier: &EnvironmentTier,
    git_repo: &str,
    git_commit_sha: &str,
) -> Result<PublishedEnvironmentDef, ApiError> {
    match tier {
        EnvironmentTier::BaseLockfile { base, lockfile } => {
            let bytes = state
                .git
                .get_file(git_repo, git_commit_sha, lockfile)
                .await
                .map_err(|e| map_environment_file_error(env_name, lockfile, e))?;
            let lockfile_content = String::from_utf8(bytes).map_err(|_| {
                ApiError::BadRequest(format!(
                    "Environment '{}' lockfile '{}' is not valid UTF-8",
                    env_name, lockfile
                ))
            })?;

            Ok(PublishedEnvironmentDef::BaseLockfile {
                base: base.clone(),
                lockfile_path: lockfile.clone(),
                lockfile_content,
            })
        }
        EnvironmentTier::Dockerfile { dockerfile } => {
            let bytes = state
                .git
                .get_file(git_repo, git_commit_sha, dockerfile)
                .await
                .map_err(|e| map_environment_file_error(env_name, dockerfile, e))?;
            let dockerfile_content = String::from_utf8(bytes).map_err(|_| {
                ApiError::BadRequest(format!(
                    "Environment '{}' Dockerfile '{}' is not valid UTF-8",
                    env_name, dockerfile
                ))
            })?;

            Ok(PublishedEnvironmentDef::Dockerfile {
                dockerfile_path: dockerfile.clone(),
                dockerfile_content,
            })
        }
        EnvironmentTier::Prebuilt { image } => Ok(PublishedEnvironmentDef::Prebuilt {
            image: image.clone(),
        }),
    }
}

fn map_environment_file_error(env_name: &str, file_path: &str, err: anyhow::Error) -> ApiError {
    if let Some(crate::git::github::GitError::FileNotFound { .. }) =
        err.downcast_ref::<crate::git::github::GitError>()
    {
        ApiError::BadRequest(format!(
            "Environment '{}' references '{}' which does not exist at commit",
            env_name, file_path
        ))
    } else {
        ApiError::Internal(err)
    }
}

/// Asynchronously build all environments declared in an `ozzy.toml`.
///
/// Uses published environment versions, not authored `ozzy.toml` path specs.
/// Errors are logged but don't propagate — the push has already succeeded.
async fn build_environments_async(
    state: &AppState,
    environments: &BTreeMap<String, StoredEnvironmentVersion>,
) {
    use crate::environments::docker::build_environment;

    for (env_name, env_row) in environments {
        let definition: PublishedEnvironmentDef =
            match serde_json::from_value(env_row.definition.clone()) {
                Ok(definition) => definition,
                Err(err) => {
                    tracing::error!(
                        "Failed to deserialize published environment '{}@{}' for '{}': {}",
                        env_row.name,
                        env_row.version,
                        env_name,
                        err
                    );
                    continue;
                }
            };

        let env_hash = crate::environments::hash::compute_env_hash(&definition);

        match build_environment(&state.db, &state.config.compute, env_name, &definition).await {
            Ok(result) => {
                tracing::info!(
                    "Environment '{}' ({}@{}) ready: {} (type={}, {}ms)",
                    env_name,
                    env_row.name,
                    env_row.version,
                    result.image_ref,
                    result.build_type,
                    result.build_duration_ms.unwrap_or(0)
                );
            }
            Err(e) => {
                tracing::error!(
                    "Failed to build environment '{}@{}' for '{}': {}",
                    env_row.name,
                    env_row.version,
                    env_name,
                    e
                );
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
