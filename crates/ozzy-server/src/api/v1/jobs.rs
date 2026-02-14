//! Job status, output, and logs endpoints.

use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Serialize;
use uuid::Uuid;

use super::auth::ApiError;
use crate::{AppState, auth::middleware::MaybeAuthUser};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/{id}", get(get_job_status))
        .route("/{id}/output", get(get_job_output))
}

// ── Response types ──────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct JobStatusResponse {
    id: Uuid,
    status: String,
    endpoint_name: String,
    node_status: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_content_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_message: Option<String>,
    created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    completed_at: Option<String>,
}

// ── GET /v1/jobs/{id} ───────────────────────────────────────────

async fn get_job_status(
    State(state): State<AppState>,
    Path(job_id): Path<Uuid>,
    auth: MaybeAuthUser,
) -> Result<Response, ApiError> {
    let job = state
        .db
        .get_job(job_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Job not found"))?;

    // Enforce read access on the owning project
    let project = state
        .db
        .get_project_by_id(job.project_id)
        .await?
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("Job references missing project")))?;

    let owner = state
        .db
        .get_user_by_id(project.owner_id)
        .await?
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("Project has missing owner")))?;

    super::access::enforce_read_access(&state, &project, &owner.username, &project.slug, &auth)
        .await?;

    let resp = JobStatusResponse {
        id: job.id,
        status: job.status.clone(),
        endpoint_name: job.endpoint_name.clone(),
        node_status: job.node_status.clone(),
        output_hash: job.output_hash.clone(),
        output_content_type: job.output_content_type.clone(),
        error_message: job.error_message.clone(),
        created_at: job.created_at.to_rfc3339(),
        started_at: job.started_at.map(|t| t.to_rfc3339()),
        completed_at: job.completed_at.map(|t| t.to_rfc3339()),
    };

    Ok(axum::Json(resp).into_response())
}

// ── GET /v1/jobs/{id}/output ────────────────────────────────────

async fn get_job_output(
    State(state): State<AppState>,
    Path(job_id): Path<Uuid>,
    auth: MaybeAuthUser,
) -> Result<Response, ApiError> {
    let job = state
        .db
        .get_job(job_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Job not found"))?;

    // Enforce read access
    let project = state
        .db
        .get_project_by_id(job.project_id)
        .await?
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("Job references missing project")))?;

    let owner = state
        .db
        .get_user_by_id(project.owner_id)
        .await?
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("Project has missing owner")))?;

    super::access::enforce_read_access(&state, &project, &owner.username, &project.slug, &auth)
        .await?;

    // Check job is done
    match job.status.as_str() {
        "done" => {}
        "failed" => {
            return Err(ApiError::Conflict(format!(
                "Job failed: {}",
                job.error_message.as_deref().unwrap_or("unknown error")
            )));
        }
        status => {
            return Err(ApiError::Conflict(format!(
                "Job is not complete (status: {})",
                status
            )));
        }
    }

    let output_hash = job
        .output_hash
        .as_deref()
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("Done job has no output_hash")))?;

    let content_type = job
        .output_content_type
        .as_deref()
        .unwrap_or("application/octet-stream");

    let ext = extension_for_content_type(content_type);

    // Serve output from content-addressed storage (where the orchestrator stored it)
    if state.storage.has_remote() {
        let url = state
            .storage
            .presigned_get_url_with_filename(
                output_hash,
                ext,
                std::time::Duration::from_secs(3600),
                Some(&format!("{}.{}", job.endpoint_name, ext)),
            )
            .await?;

        Ok((
            StatusCode::FOUND,
            [
                ("Location", url.as_str()),
                ("X-OzzyDB-Content-Hash", output_hash),
                ("X-OzzyDB-Content-Type", content_type),
            ],
        )
            .into_response())
    } else {
        // Local fallback: proxy the bytes
        let bytes = state.storage.get(output_hash, ext).await?;
        Ok((
            StatusCode::OK,
            [
                ("Content-Type", content_type),
                ("X-OzzyDB-Content-Hash", output_hash),
            ],
            bytes,
        )
            .into_response())
    }
}

fn extension_for_content_type(content_type: &str) -> &str {
    match content_type {
        "application/vnd.apache.parquet" => "parquet",
        "text/csv" => "csv",
        "application/json" => "json",
        "application/octet-stream" => "bin",
        _ => "bin",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extension_for_content_type() {
        assert_eq!(
            extension_for_content_type("application/vnd.apache.parquet"),
            "parquet"
        );
        assert_eq!(extension_for_content_type("text/csv"), "csv");
        assert_eq!(extension_for_content_type("application/json"), "json");
        assert_eq!(
            extension_for_content_type("application/octet-stream"),
            "bin"
        );
        assert_eq!(extension_for_content_type("text/plain"), "bin");
    }
}
