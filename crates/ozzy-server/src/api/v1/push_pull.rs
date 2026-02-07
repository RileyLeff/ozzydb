//! Push/pull API endpoints for syncing projects.

use axum::{
    Json, Router,
    body::Body,
    extract::{Multipart, Path, Query, State},
    http::header,
    response::Response,
    routing::{get, post},
};
use ozzy_core::registry::protocol::{
    ContentCheckRequest, ContentCheckResponse, EndpointManifest, PullManifest, PushResponse,
    ResolveEndpointResponse,
};
use parquet::file::reader::{FileReader, SerializedFileReader};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Component, Path as FsPath, PathBuf};

use super::access::{enforce_read_access, enforce_write_access};
use super::auth::ApiError;
use crate::{
    AppState,
    auth::middleware::{AuthUser, MaybeAuthUser, ScopeAction, WriteAuthUser, has_project_scope},
};

/// Extract the field name from a Content-Disposition header, including RFC 5987
/// extended syntax (`name*=utf-8''data%2Fraw.parquet`).
fn parse_content_disposition_name(headers: &axum::http::HeaderMap) -> Option<String> {
    let cd = headers.get("content-disposition")?.to_str().ok()?;
    // Try RFC 5987: name*=charset'language'value
    if let Some(idx) = cd.find("name*=") {
        let rest = &cd[idx + 6..];
        // Skip charset and language: e.g. "utf-8''data%2Fraw.parquet"
        let value = rest.split("''").nth(1)?;
        // Take until ';' or end of string, then percent-decode
        let encoded = value.split(';').next().unwrap_or(value).trim();
        let decoded = percent_decode(encoded);
        return Some(decoded);
    }
    None
}

/// Simple percent decoding (e.g., %2F → /).
fn percent_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                result.push(byte as char);
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Parse an "endpoint@ref" combined path parameter into (endpoint, ref).
fn parse_endpoint_ref(combined: &str) -> Result<(&str, &str), ApiError> {
    combined
        .split_once('@')
        .ok_or_else(|| ApiError::bad_request("Expected format: endpoint@ref"))
}

/// Extract Arrow schema from parquet bytes as a JSON value.
fn extract_parquet_schema(data: &[u8]) -> Result<serde_json::Value, anyhow::Error> {
    let reader = SerializedFileReader::new(bytes::Bytes::from(data.to_vec()))
        .map_err(|e| anyhow::anyhow!("Failed to read parquet file: {}", e))?;
    let schema = reader.metadata().file_metadata().schema_descr();
    let mut fields = Vec::new();
    for col in schema.columns() {
        fields.push(serde_json::json!({
            "name": col.name(),
            "type": format!("{:?}", col.physical_type()),
            "nullable": col.self_type().is_optional(),
        }));
    }
    Ok(serde_json::json!({ "fields": fields }))
}

/// Normalize ref name by stripping refs/heads/ or refs/tags/ prefix.
fn normalize_ref_name(ref_name: &str) -> &str {
    ref_name
        .strip_prefix("refs/heads/")
        .or_else(|| ref_name.strip_prefix("refs/tags/"))
        .unwrap_or(ref_name)
}

/// Recompute the canonical commit hash for integrity verification.
fn expected_commit_hash(commit: &ozzy_core::project::Commit) -> String {
    ozzy_core::commit::compute_commit_hash(commit)
}

/// Sanitize a value for use in HTTP headers (strip control chars and quotes).
fn sanitize_header_value(s: &str) -> String {
    s.chars().filter(|c| !c.is_control() && *c != '"').collect()
}

/// Validate and sanitize a relative path to prevent traversal.
fn sanitize_relative_path(path: &str) -> Result<String, anyhow::Error> {
    if path.contains('\0') {
        anyhow::bail!("Invalid path: null bytes not allowed");
    }

    // Normalize separators BEFORE validation to prevent backslash-encoded traversal
    let normalized = path.replace('\\', "/");

    let fs_path = FsPath::new(&normalized);
    let mut sanitized = PathBuf::new();
    for component in fs_path.components() {
        match component {
            Component::Normal(part) => sanitized.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                anyhow::bail!("Invalid path: traversal or absolute paths are not allowed");
            }
        }
    }

    if sanitized.as_os_str().is_empty() {
        anyhow::bail!("Invalid path: empty path is not allowed");
    }

    Ok(sanitized.to_string_lossy().replace('\\', "/"))
}

pub fn router() -> Router<AppState> {
    Router::new()
        // Push/pull endpoints
        .route("/{owner}/{project}/push", post(push))
        .route("/{owner}/{project}/pull", get(pull))
        .route("/{owner}/{project}/pull/manifest", get(pull_manifest))
        // Content check for deduplication
        .route("/{owner}/{project}/content/check", post(check_content))
        // Ref resolution (endpoint_ref = "endpoint@ref")
        .route(
            "/resolve/{owner}/{project}/{endpoint_ref}",
            get(resolve_endpoint),
        )
        // Endpoint fetch (endpoint_ref = "endpoint@ref")
        .route("/{owner}/{project}/{endpoint_ref}", get(fetch_endpoint))
        .route(
            "/{owner}/{project}/{endpoint_ref}/manifest",
            get(fetch_endpoint_manifest),
        )
}

#[derive(Deserialize)]
struct RefQuery {
    #[serde(rename = "ref")]
    ref_name: Option<String>,
}

/// Push a commit with data and transforms.
async fn push(
    WriteAuthUser { user, scopes }: WriteAuthUser,
    State(state): State<AppState>,
    Path((owner, project_slug)): Path<(String, String)>,
    mut multipart: Multipart,
) -> Result<Json<PushResponse>, ApiError> {
    // Existing project: require project write permission. New project: only owner can create.
    let project = if let Some(existing) = state.db.get_project(&owner, &project_slug).await? {
        enforce_write_access(&state, &existing, &owner, &project_slug, &user, &scopes).await?;
        existing
    } else {
        if owner != user.username {
            return Err(ApiError::forbidden(
                "Cannot create a project for another user",
            ));
        }
        if !has_project_scope(&scopes, ScopeAction::Write, &owner, &project_slug) {
            return Err(ApiError::forbidden(
                "Token lacks write scope for this project",
            ));
        }
        state
            .db
            .get_or_create_project(user.id, &project_slug, "private")
            .await?
    };

    let mut commit_json: Option<serde_json::Value> = None;
    let mut data_files: HashMap<String, Vec<u8>> = HashMap::new();
    let mut transform_files: HashMap<String, Vec<u8>> = HashMap::new();
    let mut lockfiles: HashMap<String, Vec<u8>> = HashMap::new();

    // Enforce upload size limit
    let max_upload = state.config.max_upload_size_bytes;
    let mut total_upload_size: u64 = 0;

    // Process multipart fields
    while let Some(field) = multipart.next_field().await? {
        // Some HTTP clients (reqwest 0.12+) encode field names containing
        // special characters using RFC 5987 extended syntax:
        //   name*=utf-8''data%2Fraw.parquet
        // axum/multer only parses standard `name="..."`, so we fall back to
        // parsing the Content-Disposition header manually.
        let name = match field.name() {
            Some(n) if !n.is_empty() => n.to_string(),
            _ => parse_content_disposition_name(field.headers()).unwrap_or_default(),
        };
        let data = field.bytes().await?;

        total_upload_size += data.len() as u64;
        if total_upload_size > max_upload {
            return Err(ApiError::bad_request(format!(
                "Upload size exceeds limit of {} bytes",
                max_upload
            )));
        }

        if name == "commit" {
            commit_json = Some(serde_json::from_slice(&data)?);
        } else if name.starts_with("data/") {
            let filename = name.strip_prefix("data/").unwrap();
            let safe_name = sanitize_relative_path(filename)?;
            data_files.insert(safe_name, data.to_vec());
        } else if name.starts_with("transforms/") {
            let filename = name.strip_prefix("transforms/").unwrap();
            let safe_name = sanitize_relative_path(filename)?;
            transform_files.insert(safe_name, data.to_vec());
        } else if name.starts_with("lockfiles/") {
            let filename = name.strip_prefix("lockfiles/").unwrap();
            let safe_name = sanitize_relative_path(filename)?;
            lockfiles.insert(safe_name, data.to_vec());
        }
    }

    let commit_data = commit_json.ok_or_else(|| anyhow::anyhow!("Missing commit data"))?;
    let mut commit: ozzy_core::project::Commit = serde_json::from_value(commit_data.clone())
        .map_err(|e| anyhow::anyhow!("Invalid commit format: {}", e))?;

    // Validate and normalize committed file paths.
    for ds in commit.data_sources.values_mut() {
        let safe_path = sanitize_relative_path(&ds.path)?;
        if !safe_path.starts_with("data/") {
            return Err(ApiError::bad_request(format!(
                "Data source '{}' has invalid path '{}': must be under data/",
                ds.name, ds.path
            )));
        }
        ds.path = safe_path;
    }
    for transform in commit.transforms.values_mut() {
        let safe_path = sanitize_relative_path(&transform.source_path)?;
        if !safe_path.starts_with("transforms/") || !safe_path.ends_with(".py") {
            return Err(ApiError::bad_request(format!(
                "Transform '{}' has invalid source_path '{}': must be a .py file under transforms/",
                transform.name, transform.source_path
            )));
        }
        transform.source_path = safe_path;
    }

    let expected_hash = expected_commit_hash(&commit);
    if expected_hash != commit.hash {
        return Err(ApiError::bad_request(format!(
            "Commit hash mismatch: expected {}, got {}",
            expected_hash, commit.hash
        )));
    }

    let mut content_registrations: Vec<(String, String, String, i64)> = Vec::new();

    // Store data files and extract schemas
    let mut new_data_count: usize = 0;
    let mut new_transform_count: usize = 0;
    let mut data_schemas: HashMap<String, serde_json::Value> = HashMap::new();
    let store_result: Result<(), ApiError> = async {
        for (filename, content) in &data_files {
            let ext = std::path::Path::new(filename)
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("bin");
            let content_hash = ozzy_core::hash::blake3_hash(content);

            // Extract schema from parquet files
            if ext == "parquet" {
                let schema = extract_parquet_schema(content).map_err(|e| {
                    ApiError::bad_request(format!("Invalid parquet file '{}': {}", filename, e))
                })?;
                data_schemas.insert(content_hash.clone(), schema);
            }

            let is_new = !state.storage.exists(&content_hash, ext).await?;
            if is_new {
                state.storage.store(content, ext).await?;
                new_data_count += 1;
            }
            let storage_key = format!(
                "content/{}/{}/{}.{}",
                &content_hash[0..2],
                &content_hash[2..4],
                &content_hash,
                ext
            );
            content_registrations.push((
                content_hash,
                storage_key,
                ext.to_string(),
                content.len() as i64,
            ));
        }

        // Store transform files (canonicalize before hashing to match client-side hashing)
        for (filename, content) in &transform_files {
            let ext = std::path::Path::new(filename)
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("py");
            let source_text = std::str::from_utf8(content).map_err(|_| {
                ApiError::bad_request(format!("Transform '{}' is not valid UTF-8", filename))
            })?;
            let canonical = ozzy_core::canon::canonicalize_source(source_text);
            let canonical_bytes = canonical.as_bytes();
            let content_hash = ozzy_core::hash::blake3_hash(canonical_bytes);
            let is_new = !state.storage.exists(&content_hash, ext).await?;
            if is_new {
                state.storage.store(canonical_bytes, ext).await?;
                new_transform_count += 1;
            }
            let storage_key = format!(
                "content/{}/{}/{}.{}",
                &content_hash[0..2],
                &content_hash[2..4],
                &content_hash,
                ext
            );
            content_registrations.push((
                content_hash,
                storage_key,
                ext.to_string(),
                canonical_bytes.len() as i64,
            ));
        }

        // Store lockfiles
        for (filename, content) in &lockfiles {
            let content_hash = ozzy_core::hash::blake3_hash(content);
            let is_new = !state.storage.exists(&content_hash, "lock").await?;
            if is_new {
                state.storage.store(content, "lock").await?;
            }
            let storage_key = format!(
                "content/{}/{}/{}.lock",
                &content_hash[0..2],
                &content_hash[2..4],
                &content_hash
            );
            content_registrations.push((
                content_hash,
                storage_key,
                "lock".to_string(),
                content.len() as i64,
            ));
            let _ = filename;
        }

        Ok(())
    }
    .await;

    // If storage failed, leave orphaned blobs for GC rather than deleting them.
    // Concurrent pushes may reference the same content-addressed blobs, and deleting
    // them here could cause data loss for other successful commits.
    if let Err(e) = store_result {
        tracing::warn!("push aborted after upload; leaving blobs for GC");
        return Err(e);
    }

    // new_transform_count already tracked during upload loop above

    // Ensure schema metadata is available for all committed data sources, including
    // deduplicated blobs that may not have been uploaded in this push.
    for ds in commit.data_sources.values() {
        if data_schemas.contains_key(&ds.hash) {
            continue;
        }
        if let Some(existing_schema) = state.db.get_data_schema_by_hash(&ds.hash).await? {
            data_schemas.insert(ds.hash.clone(), existing_schema);
        } else {
            return Err(ApiError::bad_request(format!(
                "Missing schema metadata for data source '{}' (hash {}). Re-upload the parquet file.",
                ds.name, ds.hash
            )));
        }
    }

    // Ensure transform source blobs exist before persisting commit metadata.
    // This prevents dangling commits that reference unavailable transform hashes.
    let empty_lock_sentinel = ozzy_core::hash::blake3_hash(b"");
    for t in commit.transforms.values() {
        if !state.storage.exists(&t.hash, "py").await? {
            return Err(ApiError::bad_request(format!(
                "Missing transform source for '{}' (hash {}). Re-upload the transform file.",
                t.name, t.hash
            )));
        }
        // Verify lockfile hash matches uploaded content if declared.
        // blake3_hash(b"") is the sentinel for "no lockfile" - skip verification for that.
        if !t.lockfile_hash.is_empty()
            && t.lockfile_hash != empty_lock_sentinel
            && !state.storage.exists(&t.lockfile_hash, "lock").await?
        {
            return Err(ApiError::bad_request(format!(
                "Missing lockfile for transform '{}' (hash {}). Re-upload the lockfile.",
                t.name, t.lockfile_hash
            )));
        }
    }

    // Get the ref name from commit data or default to main, then normalize it
    let raw_ref_name = commit_data["ref"].as_str().unwrap_or("refs/heads/main");
    let ref_name = normalize_ref_name(raw_ref_name).to_string();
    if ref_name.is_empty() {
        return Err(ApiError::bad_request("Ref name cannot be empty"));
    }
    ozzy_core::validate_safe_name(&ref_name).map_err(|e| ApiError::bad_request(e.to_string()))?;
    let ref_type = if raw_ref_name.starts_with("refs/tags/") {
        "tag"
    } else {
        "branch"
    };

    // Persist commit/ref/content metadata atomically.
    let mut tx = state
        .db
        .pool()
        .begin()
        .await
        .map_err(anyhow::Error::from)
        .map_err(ApiError::from)?;
    let commit_id = match state
        .db
        .create_commit_in_tx(&mut tx, project.id, &commit, Some(user.id), &data_schemas)
        .await
    {
        Ok(id) => id,
        Err(e) => {
            tracing::warn!("push commit persistence failed; leaving uploaded blobs for GC");
            return Err(e.into());
        }
    };

    // Update the pushed ref inside the same transaction.
    if let Err(e) = state
        .db
        .upsert_ref_in_tx(&mut tx, project.id, &ref_name, ref_type, commit_id)
        .await
    {
        tracing::warn!("push ref upsert failed; leaving uploaded blobs for GC");
        return Err(e.into());
    }

    // Register content references in the same transaction.
    for (hash, storage_key, content_type, byte_size) in &content_registrations {
        if let Err(e) = state
            .db
            .register_content_in_tx(&mut tx, hash, storage_key, content_type, *byte_size)
            .await
        {
            tracing::warn!("push content registration failed; leaving uploaded blobs for GC");
            return Err(e.into());
        }
    }

    if let Err(e) = tx.commit().await {
        tracing::warn!("push transaction commit failed; leaving uploaded blobs for GC");
        return Err(anyhow::Error::from(e).into());
    }

    // Update tags sent by client (best-effort: skip invalid or missing tags).
    if let Some(tags) = commit_data.get("tags").and_then(|v| v.as_object()) {
        for (tag_name, tag_hash_value) in tags {
            if let Err(e) = ozzy_core::validate_safe_name(tag_name) {
                eprintln!("Warning: skipping invalid tag '{}': {}", tag_name, e);
                continue;
            }
            let Some(tag_hash) = tag_hash_value.as_str() else {
                continue;
            };

            let tag_commit_id = if tag_hash == commit.hash {
                commit_id
            } else if let Some(existing_commit) =
                state.db.get_commit_by_hash(project.id, tag_hash).await?
            {
                existing_commit.id
            } else {
                continue;
            };

            if let Err(e) = state
                .db
                .upsert_ref(project.id, tag_name, "tag", tag_commit_id)
                .await
            {
                eprintln!("Warning: failed to upsert tag '{}': {}", tag_name, e);
            }
        }
    }

    Ok(Json(PushResponse {
        commit_hash: commit.hash.clone(),
        ref_name,
        new_data_count,
        new_transform_count,
    }))
}

/// Check which content hashes already exist (for deduplication).
/// Requires ownership of the project to prevent probing for other users' content.
async fn check_content(
    AuthUser { user, scopes }: AuthUser,
    State(state): State<AppState>,
    Path((owner, project_slug)): Path<(String, String)>,
    Json(req): Json<ContentCheckRequest>,
) -> Result<Json<ContentCheckResponse>, ApiError> {
    let project = state.db.get_project(&owner, &project_slug).await?;
    if let Some(project) = &project {
        enforce_write_access(&state, project, &owner, &project_slug, &user, &scopes).await?;
    } else {
        // First push for a new project: treat all submitted hashes as missing.
        if owner != user.username {
            return Err(ApiError::forbidden(
                "Cannot create a project for another user",
            ));
        }
        if !has_project_scope(&scopes, ScopeAction::Write, &owner, &project_slug) {
            return Err(ApiError::forbidden(
                "Token lacks write scope for this project",
            ));
        }

        let mut missing_data: Vec<String> = req.data_hashes.keys().cloned().collect();
        let mut missing_transforms: Vec<String> = req.transform_hashes.keys().cloned().collect();
        missing_data.sort();
        missing_transforms.sort();
        return Ok(Json(ContentCheckResponse {
            missing_data,
            missing_transforms,
        }));
    }

    let mut missing_data = Vec::new();
    let mut missing_transforms = Vec::new();

    // Check data hashes
    for (name, hash) in &req.data_hashes {
        if !state.storage.exists(hash, "parquet").await? {
            missing_data.push(name.clone());
        }
    }

    // Check transform hashes
    for (name, hash) in &req.transform_hashes {
        if !state.storage.exists(hash, "py").await? {
            missing_transforms.push(name.clone());
        }
    }

    Ok(Json(ContentCheckResponse {
        missing_data,
        missing_transforms,
    }))
}

/// Pull manifest (metadata only, for selective pulls).
async fn pull_manifest(
    auth: MaybeAuthUser,
    State(state): State<AppState>,
    Path((owner, project_slug)): Path<(String, String)>,
    Query(query): Query<RefQuery>,
) -> Result<Json<PullManifest>, ApiError> {
    let project = state
        .db
        .get_project(&owner, &project_slug)
        .await?
        .ok_or_else(|| ApiError::not_found("Project not found"))?;

    enforce_read_access(&state, &project, &owner, &project_slug, &auth).await?;

    let ref_name = query.ref_name.as_deref().unwrap_or("main");
    let db_ref = state
        .db
        .get_ref_by_name(project.id, ref_name)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Ref not found: {}", ref_name)))?;

    let commit = state
        .db
        .get_commit_by_id(db_ref.commit_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Commit not found"))?;

    // Get data sources and transforms for this commit
    let data_sources = state.db.get_data_sources(commit.id).await?;
    let transforms = state.db.get_transforms(commit.id).await?;

    let data_hashes: HashMap<String, String> = data_sources
        .into_iter()
        .map(|ds| (ds.name, ds.content_hash))
        .collect();

    let transform_hashes: HashMap<String, String> = transforms
        .into_iter()
        .map(|t| (t.name, t.content_hash))
        .collect();

    // Calculate total size from content refs
    let all_hashes: Vec<String> = data_hashes
        .values()
        .chain(transform_hashes.values())
        .cloned()
        .collect();
    let total_size_bytes = state.db.get_content_total_size(&all_hashes).await? as u64;

    Ok(Json(PullManifest {
        commit_hash: commit.hash,
        commit_message: commit.message,
        data_hashes,
        transform_hashes,
        total_size_bytes,
    }))
}

/// Pull full project content as streaming tar.
async fn pull(
    auth: MaybeAuthUser,
    State(state): State<AppState>,
    Path((owner, project_slug)): Path<(String, String)>,
    Query(query): Query<RefQuery>,
) -> Result<Response, ApiError> {
    let project = state
        .db
        .get_project(&owner, &project_slug)
        .await?
        .ok_or_else(|| ApiError::not_found("Project not found"))?;

    enforce_read_access(&state, &project, &owner, &project_slug, &auth).await?;

    let ref_name = query.ref_name.as_deref().unwrap_or("main");
    let db_ref = state
        .db
        .get_ref_by_name(project.id, ref_name)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Ref not found: {}", ref_name)))?;

    let commit = state
        .db
        .get_commit_by_id(db_ref.commit_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Commit not found"))?;

    // Pre-check estimated content size before loading into memory
    let max_size = state.config.max_tar_size_bytes;
    let data_sources = state.db.get_data_sources(commit.id).await?;
    let transforms = state.db.get_transforms(commit.id).await?;

    let all_content_hashes: Vec<String> = data_sources
        .iter()
        .map(|ds| ds.content_hash.clone())
        .chain(transforms.iter().map(|t| t.content_hash.clone()))
        .collect();
    let estimated_size = state.db.get_content_total_size(&all_content_hashes).await? as u64;
    if estimated_size > max_size {
        return Err(ApiError::bad_request(format!(
            "Project content ({} bytes) exceeds size limit ({} bytes)",
            estimated_size, max_size
        )));
    }

    // Build tar archive in memory
    let mut total_size: u64 = 0;
    let mut tar_data = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_data);

        // Add commit.json with full commit data (data_sources, transforms, endpoints).
        // Use the preserved original timestamp string (not DB created_at) to
        // ensure the pulled commit.json can reproduce the original commit hash.
        let original_ts: serde_json::Value =
            chrono::DateTime::parse_from_rfc3339(&commit.commit_timestamp)
                .map(|dt| serde_json::json!(dt.with_timezone(&chrono::Utc)))
                .unwrap_or_else(|_| serde_json::json!(commit.created_at));
        let mut commit_data = serde_json::json!({
            "hash": commit.hash,
            "message": commit.message,
            "parent_hashes": commit.parent_hashes,
            "author": commit.author_name,
            "timestamp": original_ts,
        });

        // Include data_sources
        let ds_map: serde_json::Value = data_sources
            .iter()
            .map(|ds| {
                (
                    ds.name.clone(),
                    serde_json::json!({
                        "name": ds.name,
                        "hash": ds.content_hash,
                        "schema_hash": ds.schema_hash,
                        "path": format!("data/{}.parquet", ds.name),
                        "row_count": ds.row_count,
                        "byte_size": ds.byte_size,
                    }),
                )
            })
            .collect::<serde_json::Map<String, serde_json::Value>>()
            .into();
        commit_data["data_sources"] = ds_map;

        // Include transforms
        let t_map: serde_json::Value = transforms
            .iter()
            .map(|t| {
                let source_path = sanitize_relative_path(&t.source_storage_key)
                    .unwrap_or_else(|_| format!("transforms/{}.py", t.name));
                (
                    t.name.clone(),
                    serde_json::json!({
                        "name": t.name,
                        "hash": t.content_hash,
                        "runtime": t.runtime,
                        "source_path": source_path,
                        "function_name": t.function_name,
                        "lockfile_hash": t.lockfile_hash,
                        "params_schema": t.params_schema,
                        "reproducible": t.reproducible,
                        "input_schema": t.input_schema,
                        "output_schema": t.output_schema,
                    }),
                )
            })
            .collect::<serde_json::Map<String, serde_json::Value>>()
            .into();
        commit_data["transforms"] = t_map;

        // Include endpoints
        let endpoints = state.db.get_endpoints(commit.id).await?;
        let e_map: serde_json::Value = endpoints
            .iter()
            .map(|e| (e.name.clone(), e.definition.clone()))
            .collect::<serde_json::Map<String, serde_json::Value>>()
            .into();
        commit_data["endpoints"] = e_map;

        let commit_json = serde_json::to_vec_pretty(&commit_data)?;
        total_size += commit_json.len() as u64;
        let mut header = tar::Header::new_gnu();
        header.set_size(commit_json.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, "commit.json", commit_json.as_slice())?;

        // Add data files (already fetched above for size estimation)
        for ds in &data_sources {
            let content = state.storage.get(&ds.content_hash, "parquet").await?;
            total_size += content.len() as u64;
            if total_size > max_size {
                return Err(ApiError::bad_request(format!(
                    "Archive size exceeds limit of {} bytes",
                    max_size
                )));
            }
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(
                &mut header,
                format!("data/{}.parquet", ds.name),
                &content[..],
            )?;
        }

        // Add transform files and their lockfiles (already fetched above)
        let mut seen_transform_paths = std::collections::HashSet::new();
        let mut seen_lockfile_paths = std::collections::HashSet::new();
        for t in &transforms {
            let source_path = sanitize_relative_path(&t.source_storage_key).map_err(|e| {
                ApiError::bad_request(format!("Invalid transform path in commit: {}", e))
            })?;
            if !source_path.starts_with("transforms/") {
                return Err(ApiError::bad_request(format!(
                    "Invalid transform path '{}' (must be under transforms/)",
                    source_path
                )));
            }

            let content = state.storage.get(&t.content_hash, "py").await?;
            total_size += content.len() as u64;
            if total_size > max_size {
                return Err(ApiError::bad_request(format!(
                    "Archive size exceeds limit of {} bytes",
                    max_size
                )));
            }
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            if seen_transform_paths.insert(source_path.clone()) {
                builder.append_data(&mut header, &source_path, &content[..])?;
            }

            // Include lockfile if it exists and hasn't been included yet
            if !t.lockfile_hash.is_empty() {
                if let Ok(lockfile_content) = state.storage.get(&t.lockfile_hash, "lock").await {
                    let lockfile_path = FsPath::new(&source_path)
                        .parent()
                        .map(|p| p.join("uv.lock"))
                        .unwrap_or_else(|| PathBuf::from("uv.lock"))
                        .to_string_lossy()
                        .replace('\\', "/");
                    if seen_lockfile_paths.insert(lockfile_path.clone()) {
                        total_size += lockfile_content.len() as u64;
                        let mut lf_header = tar::Header::new_gnu();
                        lf_header.set_size(lockfile_content.len() as u64);
                        lf_header.set_mode(0o644);
                        lf_header.set_cksum();
                        builder.append_data(
                            &mut lf_header,
                            lockfile_path,
                            &lockfile_content[..],
                        )?;
                    }
                }
            }
        }

        builder.finish()?;
    }

    Response::builder()
        .header(header::CONTENT_TYPE, "application/x-tar")
        .header(
            header::CONTENT_DISPOSITION,
            format!(
                "attachment; filename=\"{}-{}.tar\"",
                sanitize_header_value(&project_slug),
                sanitize_header_value(&ref_name)
            ),
        )
        .body(Body::from(tar_data))
        .map_err(|e| ApiError::from(anyhow::anyhow!("Failed to build response: {}", e)))
}

/// Fetch endpoint manifest.
async fn fetch_endpoint_manifest(
    auth: MaybeAuthUser,
    State(state): State<AppState>,
    Path((owner, project_slug, endpoint_ref)): Path<(String, String, String)>,
) -> Result<Json<EndpointManifest>, ApiError> {
    let (endpoint_name, ref_name) = parse_endpoint_ref(&endpoint_ref)?;
    let project = state
        .db
        .get_project(&owner, &project_slug)
        .await?
        .ok_or_else(|| ApiError::not_found("Project not found"))?;

    enforce_read_access(&state, &project, &owner, &project_slug, &auth).await?;

    let db_ref = state
        .db
        .get_ref_by_name(project.id, &ref_name)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Ref not found: {}", ref_name)))?;

    let commit = state
        .db
        .get_commit_by_id(db_ref.commit_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Commit not found"))?;

    let endpoint = state
        .db
        .get_endpoint(commit.id, &endpoint_name)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Endpoint not found: {}", endpoint_name)))?;

    // Get all data sources and transforms needed for this endpoint
    let data_sources = state.db.get_data_sources(commit.id).await?;
    let transforms = state.db.get_transforms(commit.id).await?;

    let data_hashes: HashMap<String, String> = data_sources
        .into_iter()
        .map(|ds| (ds.name, ds.content_hash))
        .collect();

    let transform_hashes: HashMap<String, String> = transforms
        .into_iter()
        .map(|t| (t.name, t.content_hash))
        .collect();

    // Calculate total size from content refs
    let all_hashes: Vec<String> = data_hashes
        .values()
        .chain(transform_hashes.values())
        .cloned()
        .collect();
    let total_size_bytes = state.db.get_content_total_size(&all_hashes).await? as u64;

    Ok(Json(EndpointManifest {
        commit_hash: commit.hash,
        endpoint: endpoint.definition,
        data_hashes,
        transform_hashes,
        total_size_bytes,
    }))
}

/// Resolve endpoint metadata for an endpoint@ref without downloading content.
async fn resolve_endpoint(
    auth: MaybeAuthUser,
    State(state): State<AppState>,
    Path((owner, project_slug, endpoint_ref)): Path<(String, String, String)>,
) -> Result<Json<ResolveEndpointResponse>, ApiError> {
    let (endpoint_name, ref_name) = parse_endpoint_ref(&endpoint_ref)?;
    let project = state
        .db
        .get_project(&owner, &project_slug)
        .await?
        .ok_or_else(|| ApiError::not_found("Project not found"))?;

    enforce_read_access(&state, &project, &owner, &project_slug, &auth).await?;

    let db_ref = state
        .db
        .get_ref_by_name(project.id, &ref_name)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Ref not found: {}", ref_name)))?;

    let commit = state
        .db
        .get_commit_by_id(db_ref.commit_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Commit not found"))?;

    let endpoint = state
        .db
        .get_endpoint(commit.id, &endpoint_name)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Endpoint not found: {}", endpoint_name)))?;

    Ok(Json(ResolveEndpointResponse {
        owner,
        project: project_slug,
        endpoint: endpoint_name.to_string(),
        ref_name: ref_name.to_string(),
        commit_hash: commit.hash,
        definition: endpoint.definition,
    }))
}

/// Fetch endpoint content as streaming tar.
async fn fetch_endpoint(
    auth: MaybeAuthUser,
    State(state): State<AppState>,
    Path((owner, project_slug, endpoint_ref)): Path<(String, String, String)>,
) -> Result<Response, ApiError> {
    let (endpoint_name, ref_name) = parse_endpoint_ref(&endpoint_ref)?;
    let project = state
        .db
        .get_project(&owner, &project_slug)
        .await?
        .ok_or_else(|| ApiError::not_found("Project not found"))?;

    enforce_read_access(&state, &project, &owner, &project_slug, &auth).await?;

    let db_ref = state
        .db
        .get_ref_by_name(project.id, &ref_name)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Ref not found: {}", ref_name)))?;

    let commit = state
        .db
        .get_commit_by_id(db_ref.commit_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Commit not found"))?;

    let endpoint = state
        .db
        .get_endpoint(commit.id, &endpoint_name)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Endpoint not found: {}", endpoint_name)))?;

    // Track total size for memory protection
    let max_size = state.config.max_tar_size_bytes;
    let mut total_size: u64 = 0;

    // Build tar archive
    let mut tar_data = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_data);

        // Add endpoint definition
        let endpoint_json = serde_json::to_vec_pretty(&endpoint.definition)?;
        total_size += endpoint_json.len() as u64;
        let mut header = tar::Header::new_gnu();
        header.set_size(endpoint_json.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, "endpoint.json", endpoint_json.as_slice())?;

        // Add data files
        let data_sources = state.db.get_data_sources(commit.id).await?;
        for ds in data_sources {
            let content = state.storage.get(&ds.content_hash, "parquet").await?;
            total_size += content.len() as u64;
            if total_size > max_size {
                return Err(ApiError::bad_request(format!(
                    "Archive size exceeds limit of {} bytes",
                    max_size
                )));
            }
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(
                &mut header,
                format!("data/{}.parquet", ds.name),
                &content[..],
            )?;
        }

        // Add transform files
        let mut seen_transform_paths = std::collections::HashSet::new();
        let mut seen_lockfile_paths = std::collections::HashSet::new();
        let transforms = state.db.get_transforms(commit.id).await?;
        for t in transforms {
            let source_path = sanitize_relative_path(&t.source_storage_key).map_err(|e| {
                ApiError::bad_request(format!("Invalid transform path in commit: {}", e))
            })?;
            if !source_path.starts_with("transforms/") {
                return Err(ApiError::bad_request(format!(
                    "Invalid transform path '{}' (must be under transforms/)",
                    source_path
                )));
            }

            let content = state.storage.get(&t.content_hash, "py").await?;
            total_size += content.len() as u64;
            if total_size > max_size {
                return Err(ApiError::bad_request(format!(
                    "Archive size exceeds limit of {} bytes",
                    max_size
                )));
            }
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            if seen_transform_paths.insert(source_path.clone()) {
                builder.append_data(&mut header, &source_path, &content[..])?;
            }

            // Include lockfile if available.
            if !t.lockfile_hash.is_empty() {
                if let Ok(lockfile_content) = state.storage.get(&t.lockfile_hash, "lock").await {
                    let lockfile_path = FsPath::new(&source_path)
                        .parent()
                        .map(|p| p.join("uv.lock"))
                        .unwrap_or_else(|| PathBuf::from("uv.lock"))
                        .to_string_lossy()
                        .replace('\\', "/");
                    if seen_lockfile_paths.insert(lockfile_path.clone()) {
                        total_size += lockfile_content.len() as u64;
                        if total_size > max_size {
                            return Err(ApiError::bad_request(format!(
                                "Archive size exceeds limit of {} bytes",
                                max_size
                            )));
                        }
                        let mut lf_header = tar::Header::new_gnu();
                        lf_header.set_size(lockfile_content.len() as u64);
                        lf_header.set_mode(0o644);
                        lf_header.set_cksum();
                        builder.append_data(
                            &mut lf_header,
                            lockfile_path,
                            &lockfile_content[..],
                        )?;
                    }
                }
            }
        }

        builder.finish()?;
    }

    Response::builder()
        .header(header::CONTENT_TYPE, "application/x-tar")
        .header(
            header::CONTENT_DISPOSITION,
            format!(
                "attachment; filename=\"{}-{}-{}.tar\"",
                sanitize_header_value(&project_slug),
                sanitize_header_value(&endpoint_name),
                sanitize_header_value(&ref_name)
            ),
        )
        .body(Body::from(tar_data))
        .map_err(|e| ApiError::from(anyhow::anyhow!("Failed to build response: {}", e)))
}
