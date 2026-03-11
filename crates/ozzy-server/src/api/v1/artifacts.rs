//! Artifact and conformance APIs.

use axum::{
    Json, Router,
    body::Body,
    extract::{Multipart, Path, State},
    http::header,
    response::Response,
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use ozzy_types::parse::parse_type_ref;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::access::{enforce_read_access, enforce_write_access};
use super::auth::ApiError;
use crate::AppState;
use crate::auth::middleware::{AuthUser, MaybeAuthUser};
use crate::db::Project;
use crate::db::v4::{
    StoredArtifact, StoredConformanceRecord, StoredTypeVersion, StoredVerificationAttempt,
};
use crate::registry::{PublishedProjectRevision, load_published_project_revision_by_commit};
use crate::verification::ensure_conformance_verified;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/{owner}/{slug}/upload", post(upload_artifact))
        .route("/{owner}/{slug}/manifest", post(create_manifest_artifact))
        .route("/{owner}/{slug}", get(list_artifacts))
        .route(
            "/{owner}/{slug}/{artifact_id}/download",
            get(download_artifact),
        )
        .route("/{owner}/{slug}/{artifact_id}", get(get_artifact))
        .route(
            "/{owner}/{slug}/{artifact_id}/conformance",
            get(get_artifact_conformance).post(declare_artifact_conformance),
        )
}

#[derive(Debug, Serialize)]
struct UploadArtifactResponse {
    artifact_id: Uuid,
    content_hash: String,
    content_type: String,
    byte_size: i64,
    deduplicated: bool,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct ArtifactSummary {
    id: Uuid,
    artifact_kind: String,
    content_hash: Option<String>,
    source_invocation_id: Option<Uuid>,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct ArtifactDetail {
    id: Uuid,
    artifact_kind: String,
    content_hash: Option<String>,
    manifest: Option<ozzy_core::artifacts::ArtifactManifest>,
    source_invocation_id: Option<Uuid>,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct ArtifactConformanceResponse {
    artifact_id: Uuid,
    records: Vec<ConformanceRecordDetail>,
}

#[derive(Debug, Deserialize)]
struct ConformanceRequest {
    #[serde(rename = "type")]
    type_ref: String,
    #[serde(default = "default_verify_conformance")]
    verify: bool,
}

fn default_verify_conformance() -> bool {
    true
}

#[derive(Debug, Serialize)]
struct ConformanceRecordDetail {
    id: Uuid,
    status: String,
    type_version: TypeVersionDetail,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    attempts: Vec<VerificationAttemptDetail>,
}

#[derive(Debug, Serialize)]
struct VerificationAttemptDetail {
    id: Uuid,
    verifier: String,
    attempt_kind: String,
    verdict: Option<String>,
    diagnostics: serde_json::Value,
    evidence: Option<serde_json::Value>,
    failure_error: Option<String>,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Clone)]
struct TypeVersionDetail {
    id: Uuid,
    name: String,
    version: String,
    canonical_type_key: String,
    expr: ozzy_types::syntax::TypeExpr,
    created_at: DateTime<Utc>,
}

async fn upload_artifact(
    AuthUser { user, scope }: AuthUser,
    State(state): State<AppState>,
    Path((owner, slug)): Path<(String, String)>,
    mut multipart: Multipart,
) -> Result<Json<UploadArtifactResponse>, ApiError> {
    let project = resolve_project_for_write(&state, &owner, &slug, &user, &scope).await?;

    let mut file_data: Option<bytes::Bytes> = None;
    let mut original_filename: Option<String> = None;
    let mut explicit_content_type: Option<String> = None;

    while let Some(field) = multipart.next_field().await? {
        let name = field.name().map(|value| value.to_string());
        match name.as_deref() {
            Some("file") => {
                original_filename = field.file_name().map(|value| value.to_string());
                file_data = Some(field.bytes().await?);
            }
            Some("content_type") => explicit_content_type = Some(field.text().await?),
            _ => {}
        }
    }

    let file_bytes = file_data.ok_or_else(|| ApiError::bad_request("Missing 'file' field"))?;
    if file_bytes.is_empty() {
        return Err(ApiError::bad_request("File is empty"));
    }

    let filename = original_filename.as_deref().unwrap_or("artifact.bin");
    let content_type = infer_content_type(filename, explicit_content_type.as_deref());
    if content_type
        .as_bytes()
        .iter()
        .any(|&byte| byte < 0x20 || byte == 0x7f)
    {
        return Err(ApiError::bad_request(format!(
            "Invalid content_type '{}': contains control characters",
            content_type
        )));
    }

    let content_hash = ozzy_core::hash::blake3_hash(&file_bytes);
    let byte_size = file_bytes.len() as i64;
    let deduplicated = state.db.get_content_ref(&content_hash).await?.is_some();
    let storage_ext = super::fetch::content_type_to_extension(&content_type);
    let r2_key = state
        .storage
        .storage_key(&content_hash, &storage_ext)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("Invalid content hash: {}", e)))?;

    if !deduplicated {
        state.storage.store(&file_bytes, &storage_ext).await?;
    }

    state
        .db
        .upsert_content_ref(&content_hash, &r2_key, &content_type, byte_size)
        .await?;

    let artifact = state
        .db
        .insert_v4_artifact(
            project.id,
            crate::db::v4::ArtifactKind::Blob,
            Some(&content_hash),
            None,
            None,
            user.id,
        )
        .await?;

    Ok(Json(UploadArtifactResponse {
        artifact_id: artifact.id,
        content_hash,
        content_type,
        byte_size,
        deduplicated,
        created_at: artifact.created_at,
    }))
}

async fn create_manifest_artifact(
    AuthUser { user, scope }: AuthUser,
    State(state): State<AppState>,
    Path((owner, slug)): Path<(String, String)>,
    Json(manifest): Json<ozzy_core::artifacts::ArtifactManifest>,
) -> Result<Json<ArtifactDetail>, ApiError> {
    let project = resolve_project_for_write(&state, &owner, &slug, &user, &scope).await?;
    let artifact = state
        .db
        .insert_v4_manifest_artifact(project.id, &manifest, None, user.id)
        .await?;
    Ok(Json(build_artifact_detail(&state, &artifact)?))
}

async fn list_artifacts(
    State(state): State<AppState>,
    Path((owner, slug)): Path<(String, String)>,
    auth: MaybeAuthUser,
) -> Result<Json<Vec<ArtifactSummary>>, ApiError> {
    let project = resolve_project_for_read(&state, &owner, &slug, &auth).await?;
    let rows = state.db.list_v4_artifacts(project.id).await?;
    Ok(Json(
        rows.into_iter()
            .map(|row| ArtifactSummary {
                id: row.id,
                artifact_kind: row.artifact_kind,
                content_hash: row.content_hash,
                source_invocation_id: row.source_invocation_id,
                created_at: row.created_at,
            })
            .collect(),
    ))
}

async fn get_artifact(
    State(state): State<AppState>,
    Path((owner, slug, artifact_id)): Path<(String, String, Uuid)>,
    auth: MaybeAuthUser,
) -> Result<Json<ArtifactDetail>, ApiError> {
    let project = resolve_project_for_read(&state, &owner, &slug, &auth).await?;
    let artifact = load_project_artifact(&state, project.id, artifact_id).await?;
    Ok(Json(build_artifact_detail(&state, &artifact)?))
}

async fn download_artifact(
    State(state): State<AppState>,
    Path((owner, slug, artifact_id)): Path<(String, String, Uuid)>,
    auth: MaybeAuthUser,
) -> Result<Response, ApiError> {
    let project = resolve_project_for_read(&state, &owner, &slug, &auth).await?;
    let artifact = load_project_artifact(&state, project.id, artifact_id).await?;

    if artifact.artifact_kind != "blob" {
        return Err(ApiError::bad_request(format!(
            "Artifact '{}' is not a blob artifact",
            artifact.id
        )));
    }

    let content_hash = artifact.content_hash.as_deref().ok_or_else(|| {
        ApiError::Internal(anyhow::anyhow!(
            "Blob artifact '{}' is missing content hash",
            artifact.id
        ))
    })?;
    let content_ref = state
        .db
        .get_content_ref(content_hash)
        .await?
        .ok_or_else(|| {
            ApiError::Internal(anyhow::anyhow!(
                "Blob artifact '{}' references missing content ref '{}'",
                artifact.id,
                content_hash
            ))
        })?;

    let ext = super::fetch::content_type_to_extension(&content_ref.content_type);
    let filename = format!("artifact-{}.{}", artifact.id, ext);
    let presigned_url = state
        .storage
        .presigned_get_url_with_filename(
            content_hash,
            &ext,
            std::time::Duration::from_secs(3600),
            Some(&filename),
        )
        .await?;

    Response::builder()
        .status(axum::http::StatusCode::FOUND)
        .header(header::LOCATION, &presigned_url)
        .header("X-OzzyDB-Content-Hash", content_hash)
        .header("X-OzzyDB-Content-Type", &content_ref.content_type)
        .body(Body::empty())
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("Failed to build response: {}", e)))
}

async fn get_artifact_conformance(
    State(state): State<AppState>,
    Path((owner, slug, artifact_id)): Path<(String, String, Uuid)>,
    auth: MaybeAuthUser,
) -> Result<Json<ArtifactConformanceResponse>, ApiError> {
    let project = resolve_project_for_read(&state, &owner, &slug, &auth).await?;
    let artifact = load_project_artifact(&state, project.id, artifact_id).await?;
    let conformance_rows = state
        .db
        .list_v4_conformance_records_for_artifact(artifact.id)
        .await?;

    let mut records = Vec::with_capacity(conformance_rows.len());
    for row in conformance_rows {
        records.push(build_conformance_detail(&state, row).await?);
    }

    Ok(Json(ArtifactConformanceResponse {
        artifact_id: artifact.id,
        records,
    }))
}

async fn declare_artifact_conformance(
    AuthUser { user, scope }: AuthUser,
    State(state): State<AppState>,
    Path((owner, slug, artifact_id)): Path<(String, String, Uuid)>,
    Json(request): Json<ConformanceRequest>,
) -> Result<Json<ConformanceRecordDetail>, ApiError> {
    let project = resolve_project_for_write(&state, &owner, &slug, &user, &scope).await?;
    let artifact = load_project_artifact(&state, project.id, artifact_id).await?;
    let published = load_latest_published_revision_for_project(&state, project.id).await?;

    let type_ref = parse_type_ref(&request.type_ref).map_err(|source| {
        ApiError::bad_request(format!(
            "Invalid type reference '{}': {}",
            request.type_ref, source
        ))
    })?;
    if type_ref.version.is_none() {
        return Err(ApiError::bad_request(
            "Conformance requires a version-pinned published type reference",
        ));
    }

    let (_, type_row) = published
        .snapshot
        .resolve_type_ref(&type_ref)
        .map_err(|_| {
            ApiError::not_found(format!(
                "Type '{}' not found in latest published registry revision",
                request.type_ref
            ))
        })?;

    let conformance = match state
        .db
        .get_v4_conformance_record(artifact.id, type_row.id)
        .await?
    {
        Some(existing) => existing,
        None => {
            state
                .db
                .insert_v4_conformance_record(
                    artifact.id,
                    type_row.id,
                    ozzy_types::conformance::ConformanceStatus::Declared,
                )
                .await?
        }
    };

    let conformance = if request.verify {
        ensure_conformance_verified(
            &state,
            published.snapshot.as_ref(),
            &artifact,
            &conformance,
            &type_ref,
        )
        .await
        .map_err(|e| ApiError::Internal(e.into()))?
    } else {
        conformance
    };

    Ok(Json(build_conformance_detail(&state, conformance).await?))
}

fn build_artifact_detail(
    state: &AppState,
    artifact: &StoredArtifact,
) -> Result<ArtifactDetail, ApiError> {
    let manifest = if artifact.artifact_kind == "manifest" {
        Some(state.db.decode_v4_artifact_manifest(artifact)?)
    } else {
        None
    };

    Ok(ArtifactDetail {
        id: artifact.id,
        artifact_kind: artifact.artifact_kind.clone(),
        content_hash: artifact.content_hash.clone(),
        manifest,
        source_invocation_id: artifact.source_invocation_id,
        created_at: artifact.created_at,
    })
}

async fn build_conformance_detail(
    state: &AppState,
    row: StoredConformanceRecord,
) -> Result<ConformanceRecordDetail, ApiError> {
    let type_row = state
        .db
        .get_v4_type_version_by_id(row.type_version_id)
        .await?
        .ok_or_else(|| {
            ApiError::Internal(anyhow::anyhow!(
                "conformance record {} references missing type version {}",
                row.id,
                row.type_version_id
            ))
        })?;
    let type_version = build_type_version_detail(state, type_row).await?;
    let attempts = state.db.list_v4_verification_attempts(row.id).await?;

    Ok(ConformanceRecordDetail {
        id: row.id,
        status: row.status,
        type_version,
        created_at: row.created_at,
        updated_at: row.updated_at,
        attempts: attempts.into_iter().map(build_attempt_detail).collect(),
    })
}

fn build_attempt_detail(row: StoredVerificationAttempt) -> VerificationAttemptDetail {
    VerificationAttemptDetail {
        id: row.id,
        verifier: row.verifier,
        attempt_kind: row.attempt_kind,
        verdict: row.verdict,
        diagnostics: row.diagnostics,
        evidence: row.evidence,
        failure_error: row.failure_error,
        created_at: row.created_at,
    }
}

async fn build_type_version_detail(
    state: &AppState,
    row: StoredTypeVersion,
) -> Result<TypeVersionDetail, ApiError> {
    let canonical = state
        .db
        .get_v4_canonical_type(row.canonical_type_id)
        .await?
        .ok_or_else(|| {
            ApiError::Internal(anyhow::anyhow!(
                "missing canonical type {} for published type version {}",
                row.canonical_type_id,
                row.id
            ))
        })?;
    let expr: ozzy_types::syntax::TypeExpr =
        serde_json::from_value(row.expr).map_err(|e| ApiError::Internal(e.into()))?;
    Ok(TypeVersionDetail {
        id: row.id,
        name: row.name,
        version: row.version,
        canonical_type_key: canonical.canonical_key,
        expr,
        created_at: row.created_at,
    })
}

async fn load_project_artifact(
    state: &AppState,
    project_id: Uuid,
    artifact_id: Uuid,
) -> Result<StoredArtifact, ApiError> {
    let artifact = state
        .db
        .get_v4_artifact(artifact_id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Artifact '{}' not found", artifact_id)))?;
    if artifact.project_id != project_id {
        return Err(ApiError::not_found(format!(
            "Artifact '{}' not found",
            artifact_id
        )));
    }
    Ok(artifact)
}

async fn resolve_project_for_read(
    state: &AppState,
    owner: &str,
    slug: &str,
    auth: &MaybeAuthUser,
) -> Result<Project, ApiError> {
    let project = state
        .db
        .get_project(owner, slug)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Project '{owner}/{slug}' not found")))?;
    enforce_read_access(state, &project, owner, slug, auth).await?;
    Ok(project)
}

async fn resolve_project_for_write(
    state: &AppState,
    owner: &str,
    slug: &str,
    user: &crate::db::User,
    scope: &str,
) -> Result<Project, ApiError> {
    let project = state
        .db
        .get_project(owner, slug)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Project '{owner}/{slug}' not found")))?;
    enforce_write_access(state, &project, owner, slug, user, scope).await?;
    Ok(project)
}

async fn load_latest_published_revision_for_project(
    state: &AppState,
    project_id: Uuid,
) -> Result<PublishedProjectRevision, ApiError> {
    let commit = state
        .db
        .list_commits(project_id, 1)
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| {
            ApiError::not_found("No commits found. Push a commit first with `ozzy push`.")
        })?;
    load_published_project_revision_by_commit(&state.db, &state.registry_snapshots, commit.id)
        .await
        .map_err(|e| ApiError::Internal(e.into()))
}

fn infer_content_type(filename: &str, explicit: Option<&str>) -> String {
    if let Some(value) = explicit {
        if !value.is_empty() {
            return value.to_string();
        }
    }
    match filename
        .rsplit('.')
        .next()
        .map(|ext| ext.to_lowercase())
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
