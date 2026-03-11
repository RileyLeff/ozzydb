//! Registry object inspection APIs.

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::get,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::access::enforce_read_access;
use super::auth::ApiError;
use crate::AppState;
use crate::auth::middleware::MaybeAuthUser;
use crate::db::Project;
use crate::db::v4::{
    StoredCanonicalType, StoredEnvironmentVersion, StoredTransformPort, StoredTransformVersion,
    StoredTypeVersion,
};

pub fn types_router() -> Router<AppState> {
    Router::new()
        .route("/{owner}/{slug}", get(list_types))
        .route("/{owner}/{slug}/resolve", get(get_type))
}

pub fn environments_router() -> Router<AppState> {
    Router::new()
        .route("/{owner}/{slug}", get(list_environments))
        .route("/{owner}/{slug}/resolve", get(get_environment))
}

pub fn transforms_router() -> Router<AppState> {
    Router::new()
        .route("/{owner}/{slug}", get(list_transforms))
        .route("/{owner}/{slug}/resolve", get(get_transform))
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResolveQuery {
    name: String,
    version: String,
}

#[derive(Debug, Serialize)]
struct TypeVersionSummary {
    id: Uuid,
    name: String,
    version: String,
    canonical_type_key: String,
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

#[derive(Debug, Serialize)]
struct EnvironmentVersionSummary {
    id: Uuid,
    name: String,
    version: String,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Clone)]
struct EnvironmentVersionDetail {
    id: Uuid,
    name: String,
    version: String,
    definition: ozzy_core::toml_spec::PublishedEnvironmentDef,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct TransformVersionSummary {
    id: Uuid,
    name: String,
    version: String,
    environment: EnvironmentVersionRef,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Clone)]
struct EnvironmentVersionRef {
    id: Uuid,
    name: String,
    version: String,
}

#[derive(Debug, Serialize, Clone)]
struct TypedPortDetail {
    name: String,
    description: Option<String>,
    #[serde(rename = "type")]
    ty: TypeVersionDetail,
}

#[derive(Debug, Serialize)]
struct TransformVersionDetail {
    id: Uuid,
    name: String,
    version: String,
    environment: EnvironmentVersionDetail,
    source_ref: Option<String>,
    command: Option<String>,
    description: Option<String>,
    params_schema: serde_json::Value,
    network_access: bool,
    secrets: Vec<String>,
    inputs: Vec<TypedPortDetail>,
    outputs: Vec<TypedPortDetail>,
    created_at: DateTime<Utc>,
}

async fn list_types(
    State(state): State<AppState>,
    Path((owner, slug)): Path<(String, String)>,
    Query(query): Query<ListQuery>,
    auth: MaybeAuthUser,
) -> Result<Json<Vec<TypeVersionSummary>>, ApiError> {
    let project = resolve_project_for_read(&state, &owner, &slug, &auth).await?;
    let mut rows = state.db.list_v4_type_versions(project.id).await?;
    if let Some(name) = query.name.as_deref() {
        rows.retain(|row| row.name == name);
    }

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let canonical = load_canonical_type(&state, row.canonical_type_id).await?;
        out.push(TypeVersionSummary {
            id: row.id,
            name: row.name,
            version: row.version,
            canonical_type_key: canonical.canonical_key,
            created_at: row.created_at,
        });
    }

    Ok(Json(out))
}

async fn get_type(
    State(state): State<AppState>,
    Path((owner, slug)): Path<(String, String)>,
    Query(query): Query<ResolveQuery>,
    auth: MaybeAuthUser,
) -> Result<Json<TypeVersionDetail>, ApiError> {
    let project = resolve_project_for_read(&state, &owner, &slug, &auth).await?;
    let row = state
        .db
        .get_v4_type_version(project.id, &query.name, &query.version)
        .await?
        .ok_or_else(|| {
            ApiError::not_found(format!("Type '{}@{}' not found", query.name, query.version))
        })?;

    Ok(Json(build_type_version_detail(&state, row).await?))
}

async fn list_environments(
    State(state): State<AppState>,
    Path((owner, slug)): Path<(String, String)>,
    Query(query): Query<ListQuery>,
    auth: MaybeAuthUser,
) -> Result<Json<Vec<EnvironmentVersionSummary>>, ApiError> {
    let project = resolve_project_for_read(&state, &owner, &slug, &auth).await?;
    let mut rows = state.db.list_v4_environment_versions(project.id).await?;
    if let Some(name) = query.name.as_deref() {
        rows.retain(|row| row.name == name);
    }

    Ok(Json(
        rows.into_iter()
            .map(|row| EnvironmentVersionSummary {
                id: row.id,
                name: row.name,
                version: row.version,
                created_at: row.created_at,
            })
            .collect(),
    ))
}

async fn get_environment(
    State(state): State<AppState>,
    Path((owner, slug)): Path<(String, String)>,
    Query(query): Query<ResolveQuery>,
    auth: MaybeAuthUser,
) -> Result<Json<EnvironmentVersionDetail>, ApiError> {
    let project = resolve_project_for_read(&state, &owner, &slug, &auth).await?;
    let row = state
        .db
        .get_v4_environment_version(project.id, &query.name, &query.version)
        .await?
        .ok_or_else(|| {
            ApiError::not_found(format!(
                "Environment '{}@{}' not found",
                query.name, query.version
            ))
        })?;

    Ok(Json(build_environment_detail(row)?))
}

async fn list_transforms(
    State(state): State<AppState>,
    Path((owner, slug)): Path<(String, String)>,
    Query(query): Query<ListQuery>,
    auth: MaybeAuthUser,
) -> Result<Json<Vec<TransformVersionSummary>>, ApiError> {
    let project = resolve_project_for_read(&state, &owner, &slug, &auth).await?;
    let mut rows = state.db.list_v4_transform_versions(project.id).await?;
    if let Some(name) = query.name.as_deref() {
        rows.retain(|row| row.name == name);
    }

    let environments = state
        .db
        .list_v4_environment_versions(project.id)
        .await?
        .into_iter()
        .map(|row| (row.id, row))
        .collect::<std::collections::BTreeMap<_, _>>();

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let environment = environments
            .get(&row.environment_version_id)
            .ok_or_else(|| {
                ApiError::Internal(anyhow::anyhow!(
                    "transform '{}@{}' references missing environment version {}",
                    row.name,
                    row.version,
                    row.environment_version_id
                ))
            })?;
        out.push(TransformVersionSummary {
            id: row.id,
            name: row.name,
            version: row.version,
            environment: EnvironmentVersionRef {
                id: environment.id,
                name: environment.name.clone(),
                version: environment.version.clone(),
            },
            created_at: row.created_at,
        });
    }

    Ok(Json(out))
}

async fn get_transform(
    State(state): State<AppState>,
    Path((owner, slug)): Path<(String, String)>,
    Query(query): Query<ResolveQuery>,
    auth: MaybeAuthUser,
) -> Result<Json<TransformVersionDetail>, ApiError> {
    let project = resolve_project_for_read(&state, &owner, &slug, &auth).await?;
    let row = state
        .db
        .get_v4_transform_version(project.id, &query.name, &query.version)
        .await?
        .ok_or_else(|| {
            ApiError::not_found(format!(
                "Transform '{}@{}' not found",
                query.name, query.version
            ))
        })?;

    Ok(Json(build_transform_detail(&state, &project, row).await?))
}

async fn build_type_version_detail(
    state: &AppState,
    row: StoredTypeVersion,
) -> Result<TypeVersionDetail, ApiError> {
    let canonical = load_canonical_type(state, row.canonical_type_id).await?;
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

fn build_environment_detail(
    row: StoredEnvironmentVersion,
) -> Result<EnvironmentVersionDetail, ApiError> {
    let definition =
        serde_json::from_value(row.definition).map_err(|e| ApiError::Internal(e.into()))?;
    Ok(EnvironmentVersionDetail {
        id: row.id,
        name: row.name,
        version: row.version,
        definition,
        created_at: row.created_at,
    })
}

async fn build_transform_detail(
    state: &AppState,
    project: &Project,
    row: StoredTransformVersion,
) -> Result<TransformVersionDetail, ApiError> {
    let environment_row = state
        .db
        .list_v4_environment_versions(project.id)
        .await?
        .into_iter()
        .find(|environment| environment.id == row.environment_version_id)
        .ok_or_else(|| {
            ApiError::Internal(anyhow::anyhow!(
                "transform '{}@{}' references missing environment version {}",
                row.name,
                row.version,
                row.environment_version_id
            ))
        })?;

    let ports = state.db.list_v4_transform_ports(row.id).await?;
    let (input_rows, output_rows): (Vec<_>, Vec<_>) = ports
        .into_iter()
        .partition(|port| port.port_kind == "input");

    Ok(TransformVersionDetail {
        id: row.id,
        name: row.name,
        version: row.version,
        environment: build_environment_detail(environment_row)?,
        source_ref: row.source_ref,
        command: row.command,
        description: row.description,
        params_schema: row.params_schema,
        network_access: row.network_access,
        secrets: row.secrets,
        inputs: build_port_details(state, input_rows).await?,
        outputs: build_port_details(state, output_rows).await?,
        created_at: row.created_at,
    })
}

async fn build_port_details(
    state: &AppState,
    mut rows: Vec<StoredTransformPort>,
) -> Result<Vec<TypedPortDetail>, ApiError> {
    rows.sort_by(|a, b| a.port_name.cmp(&b.port_name));
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let type_row = state
            .db
            .get_v4_type_version_by_id(row.type_version_id)
            .await?
            .ok_or_else(|| {
                ApiError::Internal(anyhow::anyhow!(
                    "transform port '{}' references missing type version {}",
                    row.port_name,
                    row.type_version_id
                ))
            })?;
        out.push(TypedPortDetail {
            name: row.port_name,
            description: row.description,
            ty: build_type_version_detail(state, type_row).await?,
        });
    }
    Ok(out)
}

async fn load_canonical_type(
    state: &AppState,
    canonical_type_id: Uuid,
) -> Result<StoredCanonicalType, ApiError> {
    state
        .db
        .get_v4_canonical_type(canonical_type_id)
        .await?
        .ok_or_else(|| {
            ApiError::Internal(anyhow::anyhow!(
                "missing canonical type {} for published type version",
                canonical_type_id
            ))
        })
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
