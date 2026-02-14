//! Data atom management API endpoints.
//!
//! Handles upload, listing, metadata, download, and yanking of data atoms.
//! Data atoms are immutable, content-addressed blobs stored in R2/local storage.

use axum::{
    Json, Router,
    body::Body,
    extract::{Multipart, Path, State},
    http::header,
    response::Response,
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use super::access::{enforce_read_access, enforce_write_access};
use super::auth::ApiError;
use crate::{
    AppState,
    auth::middleware::{AuthUser, MaybeAuthUser},
    db::queries::CollectionMutResult,
};

// ============================================================================
// Wire types
// ============================================================================

#[derive(Serialize)]
struct UploadResponse {
    name: String,
    hash: String,
    content_type: String,
    byte_size: i64,
    deduplicated: bool,
    collection_version: Option<i32>,
}

#[derive(Serialize)]
struct DataAtomListItem {
    id: Uuid,
    name: String,
    hash: String,
    content_type: String,
    byte_size: i64,
    yanked: bool,
    created_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct DataAtomDetail {
    id: Uuid,
    name: String,
    hash: String,
    content_type: String,
    byte_size: i64,
    uploaded_by: String,
    yanked: bool,
    yank_reason: Option<String>,
    yanked_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    metadata: HashMap<String, serde_json::Value>,
}

#[derive(Deserialize)]
struct YankRequest {
    reason: String,
}

#[derive(Deserialize)]
struct DescribeRequest {
    field: String,
    value: serde_json::Value,
}

#[derive(Serialize)]
struct MetadataEntryResponse {
    field: String,
    value: serde_json::Value,
    set_by: String,
    created_at: DateTime<Utc>,
}

// ============================================================================
// Helpers
// ============================================================================

/// Valid name pattern: [a-zA-Z0-9_-]+
fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Infer MIME content type from filename extension.
fn infer_content_type(filename: &str, explicit: Option<&str>) -> String {
    if let Some(ct) = explicit {
        if !ct.is_empty() {
            return ct.to_string();
        }
    }
    match filename
        .rsplit('.')
        .next()
        .map(|e| e.to_lowercase())
        .as_deref()
    {
        Some("parquet") => "application/vnd.apache.parquet",
        Some("csv") => "text/csv",
        Some("tsv") => "text/tab-separated-values",
        Some("json") => "application/json",
        Some("geojson") => "application/geo+json",
        Some("pdf") => "application/pdf",
        Some("tiff" | "tif") => "image/tiff",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("txt") => "text/plain",
        Some("xml") => "application/xml",
        Some("nc" | "netcdf") => "application/x-netcdf",
        Some("npy") => "application/x-npy",
        Some("npz") => "application/x-npz",
        Some("arrow" | "ipc") => "application/vnd.apache.arrow.stream",
        Some("feather") => "application/vnd.apache.arrow.file",
        _ => "application/octet-stream",
    }
    .to_string()
}

/// Derive filename stem (without extension or path).
fn filename_stem(filename: &str) -> &str {
    let base = filename.rsplit('/').next().unwrap_or(filename);
    base.split('.').next().unwrap_or(base)
}

/// Map content type to file extension for download filenames.
fn content_type_to_extension(content_type: &str) -> &str {
    match content_type {
        "application/vnd.apache.parquet" => "parquet",
        "text/csv" => "csv",
        "text/tab-separated-values" => "tsv",
        "application/json" => "json",
        "application/geo+json" => "geojson",
        "application/pdf" => "pdf",
        "image/tiff" => "tiff",
        "image/png" => "png",
        "image/jpeg" => "jpeg",
        "text/plain" => "txt",
        "application/xml" => "xml",
        "application/x-netcdf" => "nc",
        "application/vnd.apache.arrow.stream" => "arrow",
        "application/vnd.apache.arrow.file" => "feather",
        _ => "bin",
    }
}

// ============================================================================
// Routes
// ============================================================================

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/upload", post(upload_data))
        .route("/{owner}/{project}", get(list_data))
        .route("/{owner}/{project}/{name}", get(get_data_atom))
        .route("/{owner}/{project}/{name}/download", get(download_data))
        .route("/{owner}/{project}/{name}/yank", post(yank_data))
        .route("/{owner}/{project}/{name}/describe", post(describe_data))
        .route("/{owner}/{project}/{name}/metadata", get(get_metadata))
}

/// Upload a data atom (multipart/form-data).
///
/// Fields:
///   file (required): binary file data
///   project (required): "owner/slug"
///   name (optional): atom name (defaults to filename stem)
///   description (optional): human-readable description
///   content_type (optional): MIME type (inferred from extension if omitted)
///   tags (optional): comma-separated tags
///   collection (optional): add atom to this collection after upload
async fn upload_data(
    AuthUser { user, scope }: AuthUser,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<UploadResponse>, ApiError> {
    // Parse multipart fields
    let mut file_data: Option<bytes::Bytes> = None;
    let mut original_filename: Option<String> = None;
    let mut project_ref: Option<String> = None;
    let mut atom_name: Option<String> = None;
    let mut description: Option<String> = None;
    let mut explicit_content_type: Option<String> = None;
    let mut tags: Option<String> = None;
    let mut collection_name: Option<String> = None;

    while let Some(field) = multipart.next_field().await? {
        let name = field.name().map(|s| s.to_string());
        match name.as_deref() {
            Some("file") => {
                original_filename = field.file_name().map(|s| s.to_string());
                file_data = Some(field.bytes().await?);
            }
            Some("project") => project_ref = Some(field.text().await?),
            Some("name") => atom_name = Some(field.text().await?),
            Some("description") => description = Some(field.text().await?),
            Some("content_type") => explicit_content_type = Some(field.text().await?),
            Some("tags") => tags = Some(field.text().await?),
            Some("collection") => collection_name = Some(field.text().await?),
            _ => {} // skip unknown fields
        }
    }

    // Validate required fields
    let file_bytes = file_data.ok_or_else(|| ApiError::bad_request("Missing 'file' field"))?;
    if file_bytes.is_empty() {
        return Err(ApiError::bad_request("File is empty"));
    }
    let project_str =
        project_ref.ok_or_else(|| ApiError::bad_request("Missing 'project' field"))?;

    // Resolve project and check write access
    let (owner, slug) = project_str
        .split_once('/')
        .ok_or_else(|| ApiError::bad_request("Project must be in 'owner/slug' format"))?;
    let project = state
        .db
        .get_project(owner, slug)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Project '{}'", project_str)))?;
    enforce_write_access(&state, &project, owner, slug, &user, &scope).await?;

    // Determine atom name
    let fname = original_filename.as_deref().unwrap_or("unnamed");
    let name = atom_name.unwrap_or_else(|| filename_stem(fname).to_string());

    // Validate name format
    if !is_valid_name(&name) {
        return Err(ApiError::bad_request(format!(
            "Invalid name '{}': must match [a-zA-Z0-9_-]+",
            name
        )));
    }

    // Infer content type and validate it's a valid HTTP header value
    let content_type = infer_content_type(fname, explicit_content_type.as_deref());
    if content_type
        .as_bytes()
        .iter()
        .any(|&b| b < 0x20 || b == 0x7f)
    {
        return Err(ApiError::bad_request(format!(
            "Invalid content_type '{}': contains control characters",
            content_type
        )));
    }

    // Validate collection early (before any writes) — fixes atomicity + yanked bypass
    if let Some(coll_name) = &collection_name {
        let coll = state
            .db
            .get_collection(project.id, coll_name)
            .await?
            .ok_or_else(|| ApiError::not_found(format!("Collection '{}'", coll_name)))?;
        if coll.yanked {
            return Err(ApiError::gone(format!(
                "Collection '{}' has been yanked",
                coll_name
            )));
        }
    }

    // Compute hash and check for content dedup
    let hash = ozzy_core::hash::blake3_hash(&file_bytes);
    let byte_size = file_bytes.len() as i64;
    let existing_ref = state.db.get_content_ref(&hash).await?;
    let deduplicated = existing_ref.is_some();

    // Compute the actual storage key
    let r2_key = state
        .storage
        .storage_key(&hash, "bin")
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("Invalid hash: {}", e)))?;

    // Store content if this is a new hash
    if !deduplicated {
        state.storage.store(&file_bytes, "bin").await?;
    }

    // Insert data atom record BEFORE upserting content_refs.
    // If this fails (e.g., duplicate name), ref_count won't be incremented.
    let atom = state
        .db
        .insert_data_atom(
            project.id,
            &name,
            &hash,
            &content_type,
            byte_size,
            &r2_key,
            user.id,
        )
        .await?;

    // Upsert content_refs (increments ref_count if already exists)
    state
        .db
        .upsert_content_ref(&hash, &r2_key, &content_type, byte_size)
        .await?;

    // Append metadata if provided
    if let Some(desc) = &description {
        state
            .db
            .append_metadata(
                atom.id,
                "description",
                &serde_json::Value::String(desc.clone()),
                user.id,
            )
            .await?;
    }
    if let Some(tag_str) = &tags {
        let tag_list: Vec<String> = tag_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if !tag_list.is_empty() {
            state
                .db
                .append_metadata(atom.id, "tags", &serde_json::to_value(&tag_list)?, user.id)
                .await?;
        }
    }

    // Add to collection if specified (atomically to prevent lost updates)
    let mut collection_version = None;
    if let Some(coll_name) = &collection_name {
        let coll = state
            .db
            .get_collection(project.id, coll_name)
            .await?
            .ok_or_else(|| ApiError::not_found(format!("Collection '{}'", coll_name)))?;

        let new_member = vec![("data".to_string(), name.clone(), hash.clone())];
        match state
            .db
            .add_to_collection_atomically(project.id, coll.id, coll_name, user.id, &new_member)
            .await?
        {
            CollectionMutResult::Ok((ver, _)) => {
                collection_version = Some(ver.version_number);
            }
            CollectionMutResult::Yanked(coll_name) => {
                return Err(ApiError::gone(format!(
                    "Collection '{}' has been yanked",
                    coll_name
                )));
            }
            CollectionMutResult::CycleDetected(_) => {
                unreachable!("data atoms cannot create collection cycles")
            }
        }
    }

    Ok(Json(UploadResponse {
        name: atom.name,
        hash: atom.hash,
        content_type: atom.content_type,
        byte_size: atom.byte_size,
        deduplicated,
        collection_version,
    }))
}

/// List data atoms in a project.
async fn list_data(
    auth: MaybeAuthUser,
    State(state): State<AppState>,
    Path((owner, project_slug)): Path<(String, String)>,
) -> Result<Json<Vec<DataAtomListItem>>, ApiError> {
    let project = state
        .db
        .get_project(&owner, &project_slug)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Project '{}/{}'", owner, project_slug)))?;

    enforce_read_access(&state, &project, &owner, &project_slug, &auth).await?;

    let atoms = state.db.list_data_atoms(project.id).await?;

    let items: Vec<DataAtomListItem> = atoms
        .into_iter()
        .map(|a| DataAtomListItem {
            id: a.id,
            name: a.name,
            hash: a.hash,
            content_type: a.content_type,
            byte_size: a.byte_size,
            yanked: a.yanked,
            created_at: a.created_at,
        })
        .collect();

    Ok(Json(items))
}

/// Get data atom detail with latest metadata.
async fn get_data_atom(
    auth: MaybeAuthUser,
    State(state): State<AppState>,
    Path((owner, project_slug, name)): Path<(String, String, String)>,
) -> Result<Json<DataAtomDetail>, ApiError> {
    let project = state
        .db
        .get_project(&owner, &project_slug)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Project '{}/{}'", owner, project_slug)))?;

    enforce_read_access(&state, &project, &owner, &project_slug, &auth).await?;

    let atom = state
        .db
        .get_data_atom(project.id, &name)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Data atom '{}'", name)))?;

    // Get latest metadata for each field
    let metadata_entries = state.db.get_all_latest_metadata(atom.id).await?;
    let mut metadata = HashMap::new();
    for entry in metadata_entries {
        metadata.insert(entry.field, entry.value);
    }

    let uploaded_by = state
        .db
        .get_user_by_id(atom.uploaded_by)
        .await?
        .map(|u| u.username)
        .unwrap_or_else(|| atom.uploaded_by.to_string());

    Ok(Json(DataAtomDetail {
        id: atom.id,
        name: atom.name,
        hash: atom.hash,
        content_type: atom.content_type,
        byte_size: atom.byte_size,
        uploaded_by,
        yanked: atom.yanked,
        yank_reason: atom.yank_reason,
        yanked_at: atom.yanked_at,
        created_at: atom.created_at,
        metadata,
    }))
}

/// Download data atom content.
///
/// Returns raw bytes with appropriate Content-Type and Content-Disposition headers.
/// Returns 410 Gone if the atom has been yanked.
async fn download_data(
    auth: MaybeAuthUser,
    State(state): State<AppState>,
    Path((owner, project_slug, name)): Path<(String, String, String)>,
) -> Result<Response, ApiError> {
    let project = state
        .db
        .get_project(&owner, &project_slug)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Project '{}/{}'", owner, project_slug)))?;

    enforce_read_access(&state, &project, &owner, &project_slug, &auth).await?;

    let atom = state
        .db
        .get_data_atom(project.id, &name)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Data atom '{}'", name)))?;

    if atom.yanked {
        return Err(ApiError::gone(format!(
            "Data '{}' has been yanked. Reason: {}",
            atom.name,
            atom.yank_reason.as_deref().unwrap_or("No reason given")
        )));
    }

    // Build download filename with extension
    let ext = content_type_to_extension(&atom.content_type);
    let filename = format!("{}.{}", atom.name, ext);

    // When R2 is configured, redirect to a presigned URL (zero server bandwidth).
    // The presigned URL includes response-content-disposition so the browser
    // saves the file with a human-readable name (not the content hash).
    // Otherwise, proxy the bytes through the server (local-only dev mode).
    if state.storage.has_remote() {
        let presigned_url = state
            .storage
            .presigned_get_url_with_filename(
                &atom.hash,
                "bin",
                std::time::Duration::from_secs(3600),
                Some(&filename),
            )
            .await?;

        Response::builder()
            .status(axum::http::StatusCode::FOUND)
            .header(header::LOCATION, &presigned_url)
            .header("X-OzzyDB-Content-Hash", &atom.hash)
            .body(Body::empty())
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("Failed to build response: {}", e)))
    } else {
        let content = state.storage.get(&atom.hash, "bin").await?;

        Response::builder()
            .header(header::CONTENT_TYPE, &atom.content_type)
            .header(
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", filename),
            )
            .header("X-OzzyDB-Content-Hash", &atom.hash)
            .body(Body::from(content))
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("Failed to build response: {}", e)))
    }
}

/// Yank a data atom (soft delete with reason).
async fn yank_data(
    AuthUser { user, scope }: AuthUser,
    State(state): State<AppState>,
    Path((owner, project_slug, name)): Path<(String, String, String)>,
    Json(req): Json<YankRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if req.reason.is_empty() {
        return Err(ApiError::bad_request("Yank reason cannot be empty"));
    }

    let project = state
        .db
        .get_project(&owner, &project_slug)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Project '{}/{}'", owner, project_slug)))?;

    enforce_write_access(&state, &project, &owner, &project_slug, &user, &scope).await?;

    let yanked = state
        .db
        .yank_data_atom(project.id, &name, &req.reason)
        .await?;
    if !yanked {
        return Err(ApiError::not_found(format!("Data atom '{}'", name)));
    }

    Ok(Json(serde_json::json!({ "yanked": true, "name": name })))
}

/// Update metadata (append to log).
async fn describe_data(
    AuthUser { user, scope }: AuthUser,
    State(state): State<AppState>,
    Path((owner, project_slug, name)): Path<(String, String, String)>,
    Json(req): Json<DescribeRequest>,
) -> Result<Json<MetadataEntryResponse>, ApiError> {
    if req.field.is_empty() {
        return Err(ApiError::bad_request("Metadata field name cannot be empty"));
    }

    let project = state
        .db
        .get_project(&owner, &project_slug)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Project '{}/{}'", owner, project_slug)))?;

    enforce_write_access(&state, &project, &owner, &project_slug, &user, &scope).await?;

    let atom = state
        .db
        .get_data_atom(project.id, &name)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Data atom '{}'", name)))?;

    let entry = state
        .db
        .append_metadata(atom.id, &req.field, &req.value, user.id)
        .await?;

    let set_by = state
        .db
        .get_user_by_id(entry.set_by)
        .await?
        .map(|u| u.username)
        .unwrap_or_else(|| entry.set_by.to_string());

    Ok(Json(MetadataEntryResponse {
        field: entry.field,
        value: entry.value,
        set_by,
        created_at: entry.created_at,
    }))
}

/// Get full metadata history for a data atom.
async fn get_metadata(
    auth: MaybeAuthUser,
    State(state): State<AppState>,
    Path((owner, project_slug, name)): Path<(String, String, String)>,
) -> Result<Json<Vec<MetadataEntryResponse>>, ApiError> {
    let project = state
        .db
        .get_project(&owner, &project_slug)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Project '{}/{}'", owner, project_slug)))?;

    enforce_read_access(&state, &project, &owner, &project_slug, &auth).await?;

    let atom = state
        .db
        .get_data_atom(project.id, &name)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Data atom '{}'", name)))?;

    let entries = state.db.get_full_metadata_history(atom.id).await?;

    let mut items = Vec::with_capacity(entries.len());
    for e in entries {
        let set_by = state
            .db
            .get_user_by_id(e.set_by)
            .await?
            .map(|u| u.username)
            .unwrap_or_else(|| e.set_by.to_string());
        items.push(MetadataEntryResponse {
            field: e.field,
            value: e.value,
            set_by,
            created_at: e.created_at,
        });
    }

    Ok(Json(items))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_valid_name() {
        assert!(is_valid_name("raw_readings"));
        assert!(is_valid_name("data-2024"));
        assert!(is_valid_name("a"));
        assert!(is_valid_name("ABC123"));
        assert!(!is_valid_name(""));
        assert!(!is_valid_name("has space"));
        assert!(!is_valid_name("has.dot"));
        assert!(!is_valid_name("has:colon"));
        assert!(!is_valid_name("has/slash"));
    }

    #[test]
    fn test_infer_content_type() {
        assert_eq!(
            infer_content_type("data.parquet", None),
            "application/vnd.apache.parquet"
        );
        assert_eq!(infer_content_type("data.csv", None), "text/csv");
        assert_eq!(infer_content_type("data.json", None), "application/json");
        assert_eq!(infer_content_type("data.pdf", None), "application/pdf");
        assert_eq!(infer_content_type("data.png", None), "image/png");
        assert_eq!(
            infer_content_type("data.PARQUET", None),
            "application/vnd.apache.parquet"
        );
        assert_eq!(
            infer_content_type("data.unknown", None),
            "application/octet-stream"
        );
        // Explicit overrides inference
        assert_eq!(
            infer_content_type("data.csv", Some("application/json")),
            "application/json"
        );
        // Empty explicit falls back to inference
        assert_eq!(infer_content_type("data.csv", Some("")), "text/csv");
    }

    #[test]
    fn test_filename_stem() {
        assert_eq!(filename_stem("readings.parquet"), "readings");
        assert_eq!(filename_stem("path/to/file.csv"), "file");
        assert_eq!(filename_stem("no_extension"), "no_extension");
        assert_eq!(filename_stem("multi.dots.txt"), "multi");
    }

    #[test]
    fn test_content_type_to_extension() {
        assert_eq!(
            content_type_to_extension("application/vnd.apache.parquet"),
            "parquet"
        );
        assert_eq!(content_type_to_extension("text/csv"), "csv");
        assert_eq!(content_type_to_extension("application/pdf"), "pdf");
        assert_eq!(content_type_to_extension("image/png"), "png");
        assert_eq!(content_type_to_extension("unknown/type"), "bin");
    }
}
