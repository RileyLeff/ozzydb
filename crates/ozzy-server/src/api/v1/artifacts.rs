//! Artifact and conformance inspection APIs.

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use super::access::enforce_read_access;
use super::auth::ApiError;
use crate::AppState;
use crate::auth::middleware::MaybeAuthUser;
use crate::db::Project;
use crate::db::v4::{
    StoredArtifact, StoredConformanceRecord, StoredTypeVersion, StoredVerificationAttempt,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/{owner}/{slug}", get(list_artifacts))
        .route("/{owner}/{slug}/{artifact_id}", get(get_artifact))
        .route(
            "/{owner}/{slug}/{artifact_id}/conformance",
            get(get_artifact_conformance),
        )
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
