//! Query helpers for the v4 registry persistence layer.

use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

use ozzy_core::artifacts::{ArtifactManifest, ArtifactManifestError};
use ozzy_types::canonical::CanonicalType;
use ozzy_types::conformance::{ConformanceStatus, VerificationAttempt};
use ozzy_types::registry::TypeVersion;
use ozzy_types::verify::{VerificationReport, VerificationVerdict};

use super::models::{
    ArtifactKind, InvocationArtifactBindingKind, StoredArtifact, StoredCanonicalType,
    StoredConformanceRecord, StoredEnvironmentVersion, StoredInvocation, StoredInvocationArtifact,
    StoredProjectRevision, StoredRegistryRevision, StoredTransformPort, StoredTransformVersion,
    StoredTypeVersion, StoredVerificationAttempt, TransformPortKind,
};
use crate::db::queries::Database;

#[derive(Debug, Error)]
pub enum V4QueryError {
    #[error("duplicate {entity}: {detail}")]
    Duplicate {
        entity: &'static str,
        detail: String,
    },
    #[error("invalid query input: {0}")]
    InvalidInput(String),
    #[error("invalid artifact manifest: {0}")]
    InvalidArtifactManifest(#[from] ArtifactManifestError),
    #[error("artifact manifest references unknown artifact {artifact_id}")]
    UnknownReferencedArtifact { artifact_id: Uuid },
    #[error("artifact manifest references artifact {artifact_id} outside project {project_id}")]
    CrossProjectArtifactReference { artifact_id: Uuid, project_id: Uuid },
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

fn map_duplicate(err: sqlx::Error, entity: &'static str, detail: String) -> V4QueryError {
    match &err {
        sqlx::Error::Database(db_err) if db_err.is_unique_violation() => {
            V4QueryError::Duplicate { entity, detail }
        }
        _ => V4QueryError::Database(err),
    }
}

fn status_to_str(status: ConformanceStatus) -> &'static str {
    match status {
        ConformanceStatus::Declared => "declared",
        ConformanceStatus::Verified => "verified",
        ConformanceStatus::Rejected => "rejected",
    }
}

fn verdict_to_str(verdict: VerificationVerdict) -> &'static str {
    match verdict {
        VerificationVerdict::Verified => "verified",
        VerificationVerdict::Rejected => "rejected",
    }
}

impl Database {
    async fn validate_v4_manifest_artifact_members(
        &self,
        project_id: Uuid,
        manifest: &ArtifactManifest,
    ) -> Result<(), V4QueryError> {
        let unique_ids: Vec<Uuid> = manifest.referenced_artifact_id_set().into_iter().collect();
        if unique_ids.is_empty() {
            return Ok(());
        }

        #[derive(sqlx::FromRow)]
        struct ArtifactProjectRow {
            id: Uuid,
            project_id: Uuid,
        }

        let rows = sqlx::query_as::<_, ArtifactProjectRow>(
            r#"
            SELECT id, project_id
            FROM v4_artifacts
            WHERE id = ANY($1)
            "#,
        )
        .bind(&unique_ids)
        .fetch_all(self.pool())
        .await?;

        let row_map = rows
            .into_iter()
            .map(|row| (row.id, row.project_id))
            .collect::<std::collections::BTreeMap<_, _>>();

        for artifact_id in unique_ids {
            let Some(member_project_id) = row_map.get(&artifact_id).copied() else {
                return Err(V4QueryError::UnknownReferencedArtifact { artifact_id });
            };

            if member_project_id != project_id {
                return Err(V4QueryError::CrossProjectArtifactReference {
                    artifact_id,
                    project_id,
                });
            }
        }

        Ok(())
    }

    pub async fn insert_v4_canonical_type(
        &self,
        canonical: &CanonicalType,
    ) -> Result<StoredCanonicalType, V4QueryError> {
        let expr = serde_json::to_value(&canonical.expr)?;

        sqlx::query_as::<_, StoredCanonicalType>(
            r#"
            INSERT INTO v4_canonical_types (canonical_key, expr)
            VALUES ($1, $2)
            RETURNING *
            "#,
        )
        .bind(canonical.id.as_str())
        .bind(expr)
        .fetch_one(self.pool())
        .await
        .map_err(|err| map_duplicate(err, "canonical type", canonical.id.as_str().to_string()))
    }

    pub async fn get_v4_canonical_type_by_key(
        &self,
        canonical_key: &str,
    ) -> Result<Option<StoredCanonicalType>, V4QueryError> {
        let row = sqlx::query_as::<_, StoredCanonicalType>(
            "SELECT * FROM v4_canonical_types WHERE canonical_key = $1",
        )
        .bind(canonical_key)
        .fetch_optional(self.pool())
        .await?;
        Ok(row)
    }

    pub async fn get_v4_registry_revision(
        &self,
        registry_revision_id: Uuid,
    ) -> Result<Option<StoredRegistryRevision>, V4QueryError> {
        let row = sqlx::query_as::<_, StoredRegistryRevision>(
            "SELECT * FROM v4_registry_revisions WHERE id = $1",
        )
        .bind(registry_revision_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row)
    }

    pub async fn insert_v4_type_version(
        &self,
        project_id: Uuid,
        published_by: Uuid,
        type_version: &TypeVersion,
        canonical_type_id: Uuid,
    ) -> Result<StoredTypeVersion, V4QueryError> {
        let expr = serde_json::to_value(&type_version.expr)?;

        sqlx::query_as::<_, StoredTypeVersion>(
            r#"
            INSERT INTO v4_type_versions (
                project_id,
                name,
                version,
                canonical_type_id,
                expr,
                published_by
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING *
            "#,
        )
        .bind(project_id)
        .bind(&type_version.name)
        .bind(&type_version.version)
        .bind(canonical_type_id)
        .bind(expr)
        .bind(published_by)
        .fetch_one(self.pool())
        .await
        .map_err(|err| {
            map_duplicate(
                err,
                "type version",
                format!(
                    "{project_id}/{}@{}",
                    type_version.name, type_version.version
                ),
            )
        })
    }

    pub async fn get_v4_type_version(
        &self,
        project_id: Uuid,
        name: &str,
        version: &str,
    ) -> Result<Option<StoredTypeVersion>, V4QueryError> {
        let row = sqlx::query_as::<_, StoredTypeVersion>(
            r#"
            SELECT *
            FROM v4_type_versions
            WHERE project_id = $1 AND name = $2 AND version = $3
            "#,
        )
        .bind(project_id)
        .bind(name)
        .bind(version)
        .fetch_optional(self.pool())
        .await?;
        Ok(row)
    }

    pub async fn insert_v4_environment_version(
        &self,
        project_id: Uuid,
        published_by: Uuid,
        name: &str,
        version: &str,
        definition: Value,
    ) -> Result<StoredEnvironmentVersion, V4QueryError> {
        sqlx::query_as::<_, StoredEnvironmentVersion>(
            r#"
            INSERT INTO v4_environment_versions (
                project_id,
                name,
                version,
                definition,
                published_by
            )
            VALUES ($1, $2, $3, $4, $5)
            RETURNING *
            "#,
        )
        .bind(project_id)
        .bind(name)
        .bind(version)
        .bind(definition)
        .bind(published_by)
        .fetch_one(self.pool())
        .await
        .map_err(|err| {
            map_duplicate(
                err,
                "environment version",
                format!("{project_id}/{name}@{version}"),
            )
        })
    }

    pub async fn get_v4_environment_version(
        &self,
        project_id: Uuid,
        name: &str,
        version: &str,
    ) -> Result<Option<StoredEnvironmentVersion>, V4QueryError> {
        let row = sqlx::query_as::<_, StoredEnvironmentVersion>(
            r#"
            SELECT *
            FROM v4_environment_versions
            WHERE project_id = $1 AND name = $2 AND version = $3
            "#,
        )
        .bind(project_id)
        .bind(name)
        .bind(version)
        .fetch_optional(self.pool())
        .await?;
        Ok(row)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn insert_v4_transform_version(
        &self,
        project_id: Uuid,
        published_by: Uuid,
        name: &str,
        version: &str,
        environment_version_id: Uuid,
        source_ref: Option<&str>,
        command: Option<&str>,
        description: Option<&str>,
        params_schema: Value,
        network_access: bool,
        secrets: &[String],
    ) -> Result<StoredTransformVersion, V4QueryError> {
        sqlx::query_as::<_, StoredTransformVersion>(
            r#"
            INSERT INTO v4_transform_versions (
                project_id,
                name,
                version,
                environment_version_id,
                source_ref,
                command,
                description,
                params_schema,
                network_access,
                secrets,
                published_by
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            RETURNING *
            "#,
        )
        .bind(project_id)
        .bind(name)
        .bind(version)
        .bind(environment_version_id)
        .bind(source_ref)
        .bind(command)
        .bind(description)
        .bind(params_schema)
        .bind(network_access)
        .bind(secrets)
        .bind(published_by)
        .fetch_one(self.pool())
        .await
        .map_err(|err| {
            map_duplicate(
                err,
                "transform version",
                format!("{project_id}/{name}@{version}"),
            )
        })
    }

    pub async fn get_v4_transform_version(
        &self,
        project_id: Uuid,
        name: &str,
        version: &str,
    ) -> Result<Option<StoredTransformVersion>, V4QueryError> {
        let row = sqlx::query_as::<_, StoredTransformVersion>(
            r#"
            SELECT *
            FROM v4_transform_versions
            WHERE project_id = $1 AND name = $2 AND version = $3
            "#,
        )
        .bind(project_id)
        .bind(name)
        .bind(version)
        .fetch_optional(self.pool())
        .await?;
        Ok(row)
    }

    pub async fn insert_v4_transform_port(
        &self,
        transform_version_id: Uuid,
        port_kind: TransformPortKind,
        port_name: &str,
        type_version_id: Uuid,
        description: Option<&str>,
    ) -> Result<StoredTransformPort, V4QueryError> {
        sqlx::query_as::<_, StoredTransformPort>(
            r#"
            INSERT INTO v4_transform_ports (
                transform_version_id,
                port_kind,
                port_name,
                type_version_id,
                description
            )
            VALUES ($1, $2, $3, $4, $5)
            RETURNING *
            "#,
        )
        .bind(transform_version_id)
        .bind(port_kind.as_str())
        .bind(port_name)
        .bind(type_version_id)
        .bind(description)
        .fetch_one(self.pool())
        .await
        .map_err(|err| {
            map_duplicate(
                err,
                "transform port",
                format!(
                    "{transform_version_id}/{}/{}",
                    port_kind.as_str(),
                    port_name
                ),
            )
        })
    }

    pub async fn list_v4_transform_ports(
        &self,
        transform_version_id: Uuid,
    ) -> Result<Vec<StoredTransformPort>, V4QueryError> {
        let rows = sqlx::query_as::<_, StoredTransformPort>(
            r#"
            SELECT *
            FROM v4_transform_ports
            WHERE transform_version_id = $1
            ORDER BY port_kind, port_name
            "#,
        )
        .bind(transform_version_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows)
    }

    pub async fn insert_v4_registry_revision(
        &self,
        project_id: Uuid,
        created_by: Uuid,
    ) -> Result<StoredRegistryRevision, V4QueryError> {
        let row = sqlx::query_as::<_, StoredRegistryRevision>(
            r#"
            INSERT INTO v4_registry_revisions (project_id, created_by)
            VALUES ($1, $2)
            RETURNING *
            "#,
        )
        .bind(project_id)
        .bind(created_by)
        .fetch_one(self.pool())
        .await?;
        Ok(row)
    }

    pub async fn attach_v4_type_version_to_registry_revision(
        &self,
        registry_revision_id: Uuid,
        type_version_id: Uuid,
    ) -> Result<(), V4QueryError> {
        sqlx::query(
            r#"
            INSERT INTO v4_registry_revision_type_versions (registry_revision_id, type_version_id)
            VALUES ($1, $2)
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(registry_revision_id)
        .bind(type_version_id)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn attach_v4_environment_version_to_registry_revision(
        &self,
        registry_revision_id: Uuid,
        environment_version_id: Uuid,
    ) -> Result<(), V4QueryError> {
        sqlx::query(
            r#"
            INSERT INTO v4_registry_revision_environment_versions (
                registry_revision_id,
                environment_version_id
            )
            VALUES ($1, $2)
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(registry_revision_id)
        .bind(environment_version_id)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn attach_v4_transform_version_to_registry_revision(
        &self,
        registry_revision_id: Uuid,
        transform_version_id: Uuid,
    ) -> Result<(), V4QueryError> {
        sqlx::query(
            r#"
            INSERT INTO v4_registry_revision_transform_versions (
                registry_revision_id,
                transform_version_id
            )
            VALUES ($1, $2)
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(registry_revision_id)
        .bind(transform_version_id)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn list_v4_registry_revision_type_versions(
        &self,
        registry_revision_id: Uuid,
    ) -> Result<Vec<StoredTypeVersion>, V4QueryError> {
        let rows = sqlx::query_as::<_, StoredTypeVersion>(
            r#"
            SELECT tv.*
            FROM v4_registry_revision_type_versions rrtv
            JOIN v4_type_versions tv ON tv.id = rrtv.type_version_id
            WHERE rrtv.registry_revision_id = $1
            ORDER BY tv.name, tv.version
            "#,
        )
        .bind(registry_revision_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows)
    }

    pub async fn list_v4_registry_revision_environment_versions(
        &self,
        registry_revision_id: Uuid,
    ) -> Result<Vec<StoredEnvironmentVersion>, V4QueryError> {
        let rows = sqlx::query_as::<_, StoredEnvironmentVersion>(
            r#"
            SELECT ev.*
            FROM v4_registry_revision_environment_versions rrev
            JOIN v4_environment_versions ev ON ev.id = rrev.environment_version_id
            WHERE rrev.registry_revision_id = $1
            ORDER BY ev.name, ev.version
            "#,
        )
        .bind(registry_revision_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows)
    }

    pub async fn list_v4_registry_revision_transform_versions(
        &self,
        registry_revision_id: Uuid,
    ) -> Result<Vec<StoredTransformVersion>, V4QueryError> {
        let rows = sqlx::query_as::<_, StoredTransformVersion>(
            r#"
            SELECT tv.*
            FROM v4_registry_revision_transform_versions rrtv
            JOIN v4_transform_versions tv ON tv.id = rrtv.transform_version_id
            WHERE rrtv.registry_revision_id = $1
            ORDER BY tv.name, tv.version
            "#,
        )
        .bind(registry_revision_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows)
    }

    pub async fn list_v4_registry_revision_canonical_types(
        &self,
        registry_revision_id: Uuid,
    ) -> Result<Vec<StoredCanonicalType>, V4QueryError> {
        let rows = sqlx::query_as::<_, StoredCanonicalType>(
            r#"
            SELECT DISTINCT ct.*
            FROM v4_registry_revision_type_versions rrtv
            JOIN v4_type_versions tv ON tv.id = rrtv.type_version_id
            JOIN v4_canonical_types ct ON ct.id = tv.canonical_type_id
            WHERE rrtv.registry_revision_id = $1
            ORDER BY ct.canonical_key
            "#,
        )
        .bind(registry_revision_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows)
    }

    pub async fn list_v4_registry_revision_transform_ports(
        &self,
        registry_revision_id: Uuid,
    ) -> Result<Vec<StoredTransformPort>, V4QueryError> {
        let rows = sqlx::query_as::<_, StoredTransformPort>(
            r#"
            SELECT tp.*
            FROM v4_registry_revision_transform_versions rrtv
            JOIN v4_transform_ports tp ON tp.transform_version_id = rrtv.transform_version_id
            WHERE rrtv.registry_revision_id = $1
            ORDER BY tp.transform_version_id, tp.port_kind, tp.port_name
            "#,
        )
        .bind(registry_revision_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows)
    }

    pub async fn insert_v4_project_revision(
        &self,
        project_id: Uuid,
        source_commit_id: Uuid,
        registry_revision_id: Uuid,
        ozzy_toml_hash: &str,
        ozzy_toml_raw: &str,
        environments: Value,
        transforms: Value,
        endpoints: Value,
        project_meta: Value,
        created_by: Uuid,
    ) -> Result<StoredProjectRevision, V4QueryError> {
        sqlx::query_as::<_, StoredProjectRevision>(
            r#"
            INSERT INTO v4_project_revisions (
                project_id,
                source_commit_id,
                registry_revision_id,
                ozzy_toml_hash,
                ozzy_toml_raw,
                environments,
                transforms,
                endpoints,
                project_meta,
                created_by
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            RETURNING *
            "#,
        )
        .bind(project_id)
        .bind(source_commit_id)
        .bind(registry_revision_id)
        .bind(ozzy_toml_hash)
        .bind(ozzy_toml_raw)
        .bind(environments)
        .bind(transforms)
        .bind(endpoints)
        .bind(project_meta)
        .bind(created_by)
        .fetch_one(self.pool())
        .await
        .map_err(|err| {
            map_duplicate(
                err,
                "project revision",
                format!("{project_id}/{source_commit_id}"),
            )
        })
    }

    pub async fn get_v4_project_revision_by_commit(
        &self,
        source_commit_id: Uuid,
    ) -> Result<Option<StoredProjectRevision>, V4QueryError> {
        let row = sqlx::query_as::<_, StoredProjectRevision>(
            "SELECT * FROM v4_project_revisions WHERE source_commit_id = $1",
        )
        .bind(source_commit_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn insert_v4_invocation(
        &self,
        project_revision_id: Uuid,
        transform_version_id: Uuid,
        endpoint_name: Option<&str>,
        node_name: Option<&str>,
        params: Value,
        params_hash: &str,
        input_bindings: Value,
        output_bindings: Value,
        status: &str,
        created_by: Option<Uuid>,
    ) -> Result<StoredInvocation, V4QueryError> {
        let row = sqlx::query_as::<_, StoredInvocation>(
            r#"
            INSERT INTO v4_invocations (
                project_revision_id,
                transform_version_id,
                endpoint_name,
                node_name,
                params,
                params_hash,
                input_bindings,
                output_bindings,
                status,
                created_by
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            RETURNING *
            "#,
        )
        .bind(project_revision_id)
        .bind(transform_version_id)
        .bind(endpoint_name)
        .bind(node_name)
        .bind(params)
        .bind(params_hash)
        .bind(input_bindings)
        .bind(output_bindings)
        .bind(status)
        .bind(created_by)
        .fetch_one(self.pool())
        .await?;
        Ok(row)
    }

    pub async fn insert_v4_artifact(
        &self,
        project_id: Uuid,
        artifact_kind: ArtifactKind,
        content_hash: Option<&str>,
        manifest: Option<Value>,
        source_invocation_id: Option<Uuid>,
        created_by: Uuid,
    ) -> Result<StoredArtifact, V4QueryError> {
        let row = sqlx::query_as::<_, StoredArtifact>(
            r#"
            INSERT INTO v4_artifacts (
                project_id,
                artifact_kind,
                content_hash,
                manifest,
                source_invocation_id,
                created_by
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING *
            "#,
        )
        .bind(project_id)
        .bind(artifact_kind.as_str())
        .bind(content_hash)
        .bind(manifest)
        .bind(source_invocation_id)
        .bind(created_by)
        .fetch_one(self.pool())
        .await?;
        Ok(row)
    }

    pub async fn insert_v4_manifest_artifact(
        &self,
        project_id: Uuid,
        manifest: &ArtifactManifest,
        source_invocation_id: Option<Uuid>,
        created_by: Uuid,
    ) -> Result<StoredArtifact, V4QueryError> {
        manifest.validate()?;
        self.validate_v4_manifest_artifact_members(project_id, manifest)
            .await?;

        self.insert_v4_artifact(
            project_id,
            ArtifactKind::Manifest,
            None,
            Some(serde_json::to_value(manifest)?),
            source_invocation_id,
            created_by,
        )
        .await
    }

    pub async fn get_v4_artifact(
        &self,
        artifact_id: Uuid,
    ) -> Result<Option<StoredArtifact>, V4QueryError> {
        let row = sqlx::query_as::<_, StoredArtifact>("SELECT * FROM v4_artifacts WHERE id = $1")
            .bind(artifact_id)
            .fetch_optional(self.pool())
            .await?;
        Ok(row)
    }

    pub async fn list_v4_artifacts(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<StoredArtifact>, V4QueryError> {
        let rows = sqlx::query_as::<_, StoredArtifact>(
            r#"
            SELECT *
            FROM v4_artifacts
            WHERE project_id = $1
            ORDER BY created_at DESC, id DESC
            "#,
        )
        .bind(project_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows)
    }

    pub async fn insert_v4_invocation_artifact(
        &self,
        invocation_id: Uuid,
        binding_kind: InvocationArtifactBindingKind,
        port_name: &str,
        artifact_id: Uuid,
    ) -> Result<StoredInvocationArtifact, V4QueryError> {
        sqlx::query_as::<_, StoredInvocationArtifact>(
            r#"
            INSERT INTO v4_invocation_artifacts (
                invocation_id,
                binding_kind,
                port_name,
                artifact_id
            )
            VALUES ($1, $2, $3, $4)
            RETURNING *
            "#,
        )
        .bind(invocation_id)
        .bind(binding_kind.as_str())
        .bind(port_name)
        .bind(artifact_id)
        .fetch_one(self.pool())
        .await
        .map_err(|err| {
            map_duplicate(
                err,
                "invocation artifact binding",
                format!("{invocation_id}/{}/{port_name}", binding_kind.as_str()),
            )
        })
    }

    pub async fn list_v4_invocation_artifacts(
        &self,
        invocation_id: Uuid,
    ) -> Result<Vec<StoredInvocationArtifact>, V4QueryError> {
        let rows = sqlx::query_as::<_, StoredInvocationArtifact>(
            r#"
            SELECT *
            FROM v4_invocation_artifacts
            WHERE invocation_id = $1
            ORDER BY binding_kind, port_name
            "#,
        )
        .bind(invocation_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows)
    }

    pub async fn insert_v4_conformance_record(
        &self,
        artifact_id: Uuid,
        type_version_id: Uuid,
        status: ConformanceStatus,
    ) -> Result<StoredConformanceRecord, V4QueryError> {
        sqlx::query_as::<_, StoredConformanceRecord>(
            r#"
            INSERT INTO v4_conformance_records (artifact_id, type_version_id, status)
            VALUES ($1, $2, $3)
            RETURNING *
            "#,
        )
        .bind(artifact_id)
        .bind(type_version_id)
        .bind(status_to_str(status))
        .fetch_one(self.pool())
        .await
        .map_err(|err| {
            map_duplicate(
                err,
                "conformance record",
                format!("{artifact_id}/{type_version_id}"),
            )
        })
    }

    pub async fn get_v4_conformance_record(
        &self,
        artifact_id: Uuid,
        type_version_id: Uuid,
    ) -> Result<Option<StoredConformanceRecord>, V4QueryError> {
        let row = sqlx::query_as::<_, StoredConformanceRecord>(
            r#"
            SELECT *
            FROM v4_conformance_records
            WHERE artifact_id = $1 AND type_version_id = $2
            "#,
        )
        .bind(artifact_id)
        .bind(type_version_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row)
    }

    pub fn decode_v4_artifact_manifest(
        &self,
        artifact: &StoredArtifact,
    ) -> Result<ArtifactManifest, V4QueryError> {
        if artifact.artifact_kind != ArtifactKind::Manifest.as_str() {
            return Err(V4QueryError::InvalidInput(format!(
                "artifact {} is not a manifest artifact",
                artifact.id
            )));
        }

        let manifest = artifact.manifest.clone().ok_or_else(|| {
            V4QueryError::InvalidInput(format!(
                "manifest artifact {} is missing payload",
                artifact.id
            ))
        })?;

        Ok(serde_json::from_value(manifest)?)
    }

    pub async fn insert_v4_verification_attempt_from_report(
        &self,
        conformance_record_id: Uuid,
        report: &VerificationReport,
    ) -> Result<StoredVerificationAttempt, V4QueryError> {
        let diagnostics = serde_json::to_value(&report.diagnostics)?;

        let row = sqlx::query_as::<_, StoredVerificationAttempt>(
            r#"
            INSERT INTO v4_verification_attempts (
                conformance_record_id,
                verifier,
                attempt_kind,
                verdict,
                diagnostics,
                evidence
            )
            VALUES ($1, $2, 'completed', $3, $4, $5)
            RETURNING *
            "#,
        )
        .bind(conformance_record_id)
        .bind(&report.verifier)
        .bind(verdict_to_str(report.verdict))
        .bind(diagnostics)
        .bind(report.evidence.clone())
        .fetch_one(self.pool())
        .await?;
        Ok(row)
    }

    pub async fn insert_v4_verification_attempt_from_failure(
        &self,
        conformance_record_id: Uuid,
        attempt: &VerificationAttempt,
    ) -> Result<StoredVerificationAttempt, V4QueryError> {
        let VerificationAttempt::Failed(failure) = attempt else {
            return Err(V4QueryError::InvalidInput(
                "expected a failed verification attempt".to_string(),
            ));
        };

        let row = sqlx::query_as::<_, StoredVerificationAttempt>(
            r#"
            INSERT INTO v4_verification_attempts (
                conformance_record_id,
                verifier,
                attempt_kind,
                diagnostics,
                failure_error
            )
            VALUES ($1, $2, 'failed', '[]'::jsonb, $3)
            RETURNING *
            "#,
        )
        .bind(conformance_record_id)
        .bind(&failure.verifier)
        .bind(&failure.error)
        .fetch_one(self.pool())
        .await?;
        Ok(row)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use sqlx::postgres::PgPoolOptions;
    use std::env;

    use ozzy_types::canonical::CanonicalType;
    use ozzy_types::conformance::ConformanceStatus;
    use ozzy_types::syntax::TypeExpr;

    use super::*;

    async fn get_test_db() -> Option<Database> {
        let url = env::var("DATABASE_URL").ok()?;
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .ok()?;

        sqlx::migrate!("./migrations").run(&pool).await.ok()?;
        Some(Database::new(pool))
    }

    fn unique_name(prefix: &str) -> String {
        format!(
            "{}_{}",
            prefix,
            uuid::Uuid::new_v4().to_string().replace('-', "")[..8].to_string()
        )
    }

    #[tokio::test]
    async fn v4_registry_foundation_round_trips_against_postgres() {
        let Some(db) = get_test_db().await else {
            return;
        };

        let user = db
            .upsert_user_from_github(
                rand::random::<i64>() & i64::MAX,
                &unique_name("v4user"),
                None,
                None,
            )
            .await
            .expect("create user");

        let project = db
            .create_project(user.id, &unique_name("v4proj"), Some("v4 test"), "private")
            .await
            .expect("create project");

        let commit = db
            .insert_commit(
                project.id,
                "github",
                "rileyleff/ozzydb",
                &format!("{:040x}", rand::random::<u64>()),
                "test_ozzy_toml_hash",
                user.id,
                Some("v4 registry test"),
            )
            .await
            .expect("insert commit");

        let expr = TypeExpr::ref_("float64");
        let canonical =
            CanonicalType::canonicalize(&Default::default(), &expr).expect("canonicalize type");
        let stored_canonical = db
            .insert_v4_canonical_type(&canonical)
            .await
            .expect("insert canonical type");

        let type_version = TypeVersion::new("std/WaterPotential", "1", expr);
        let stored_type = db
            .insert_v4_type_version(project.id, user.id, &type_version, stored_canonical.id)
            .await
            .expect("insert type version");

        let stored_env = db
            .insert_v4_environment_version(
                project.id,
                user.id,
                "python_sci",
                "1",
                json!({
                    "kind": "base_lockfile",
                    "base": "python",
                    "installer": "pip_requirements",
                    "lockfile_content": "polars==1.0\n"
                }),
            )
            .await
            .expect("insert environment version");

        let stored_transform = db
            .insert_v4_transform_version(
                project.id,
                user.id,
                "clean_wp",
                "1",
                stored_env.id,
                Some("transforms/clean.py:clean"),
                None,
                Some("clean water potential data"),
                json!({}),
                false,
                &Vec::<String>::new(),
            )
            .await
            .expect("insert transform version");

        let stored_port = db
            .insert_v4_transform_port(
                stored_transform.id,
                TransformPortKind::Input,
                "raw",
                stored_type.id,
                Some("raw input data"),
            )
            .await
            .expect("insert transform port");

        let registry_revision = db
            .insert_v4_registry_revision(project.id, user.id)
            .await
            .expect("insert registry revision");
        db.attach_v4_type_version_to_registry_revision(registry_revision.id, stored_type.id)
            .await
            .expect("attach type version");
        db.attach_v4_environment_version_to_registry_revision(registry_revision.id, stored_env.id)
            .await
            .expect("attach environment version");
        db.attach_v4_transform_version_to_registry_revision(
            registry_revision.id,
            stored_transform.id,
        )
        .await
        .expect("attach transform version");

        let project_revision = db
            .insert_v4_project_revision(
                project.id,
                commit.id,
                registry_revision.id,
                "test_ozzy_toml_hash",
                "[types]\nWaterPotential = 'float64'\n",
                json!({
                    "python_sci": {
                        "name": "python_sci",
                        "version": "1"
                    }
                }),
                json!({
                    "clean_wp": {
                        "source": "transforms/clean.py:clean",
                        "environment": "python_sci@1",
                        "params": {},
                        "network": false,
                        "secrets": []
                    }
                }),
                json!({
                    "fetch_water_potential": {
                        "nodes": {
                            "clean": {
                                "transform": "clean_wp",
                                "params": {}
                            }
                        },
                        "edges": []
                    }
                }),
                json!({ "name": "test", "owner": "user" }),
                user.id,
            )
            .await
            .expect("insert project revision");

        let invocation = db
            .insert_v4_invocation(
                project_revision.id,
                stored_transform.id,
                Some("fetch_water_potential"),
                Some("clean"),
                json!({ "alpha": 1 }),
                "params_hash_1",
                json!({ "raw": "artifact_in" }),
                json!({}),
                "queued",
                Some(user.id),
            )
            .await
            .expect("insert invocation");

        db.upsert_content_ref(
            "artifact_hash_1",
            "content/ar/ti/artifact_hash_1.bin",
            "application/octet-stream",
            42,
        )
        .await
        .expect("upsert content ref");
        let blob_artifact = db
            .insert_v4_artifact(
                project.id,
                ArtifactKind::Blob,
                Some("artifact_hash_1"),
                None,
                None,
                user.id,
            )
            .await
            .expect("insert blob artifact");
        let manifest_artifact = db
            .insert_v4_manifest_artifact(
                project.id,
                &ArtifactManifest::Bundle {
                    entries: std::collections::BTreeMap::from([(
                        "raw".to_string(),
                        ozzy_core::artifacts::ArtifactManifestEntry {
                            artifact_id: blob_artifact.id,
                        },
                    )]),
                },
                Some(invocation.id),
                user.id,
            )
            .await
            .expect("insert manifest artifact");
        let input_binding = db
            .insert_v4_invocation_artifact(
                invocation.id,
                InvocationArtifactBindingKind::Input,
                "raw",
                blob_artifact.id,
            )
            .await
            .expect("insert input artifact binding");
        let output_binding = db
            .insert_v4_invocation_artifact(
                invocation.id,
                InvocationArtifactBindingKind::Output,
                "result",
                manifest_artifact.id,
            )
            .await
            .expect("insert output artifact binding");

        let conformance = db
            .insert_v4_conformance_record(
                manifest_artifact.id,
                stored_type.id,
                ConformanceStatus::Declared,
            )
            .await
            .expect("insert conformance");

        let attempt = db
            .insert_v4_verification_attempt_from_report(
                conformance.id,
                &VerificationReport {
                    verifier: "builtin.v1".to_string(),
                    verdict: VerificationVerdict::Verified,
                    diagnostics: vec![],
                    evidence: Some(json!({ "kind": "scalar" })),
                },
            )
            .await
            .expect("insert verification attempt");

        assert_eq!(stored_canonical.canonical_key, canonical.id.as_str());
        assert_eq!(stored_port.port_kind, "input");
        assert_eq!(project_revision.source_commit_id, commit.id);
        assert_eq!(invocation.status, "queued");
        assert_eq!(attempt.verdict.as_deref(), Some("verified"));

        let loaded_types = db
            .list_v4_registry_revision_type_versions(registry_revision.id)
            .await
            .expect("load revision types");
        let loaded_envs = db
            .list_v4_registry_revision_environment_versions(registry_revision.id)
            .await
            .expect("load revision envs");
        let loaded_transforms = db
            .list_v4_registry_revision_transform_versions(registry_revision.id)
            .await
            .expect("load revision transforms");
        let loaded_ports = db
            .list_v4_transform_ports(stored_transform.id)
            .await
            .expect("load transform ports");
        let loaded_project_revision = db
            .get_v4_project_revision_by_commit(commit.id)
            .await
            .expect("load project revision")
            .expect("project revision exists");
        let loaded_blob_artifact = db
            .get_v4_artifact(blob_artifact.id)
            .await
            .expect("load blob artifact")
            .expect("blob artifact exists");
        let loaded_artifacts = db
            .list_v4_artifacts(project.id)
            .await
            .expect("list v4 artifacts");
        let decoded_manifest = db
            .decode_v4_artifact_manifest(&manifest_artifact)
            .expect("decode manifest artifact");
        let loaded_invocation_artifacts = db
            .list_v4_invocation_artifacts(invocation.id)
            .await
            .expect("list invocation artifacts");
        let loaded_conformance = db
            .get_v4_conformance_record(manifest_artifact.id, stored_type.id)
            .await
            .expect("load conformance")
            .expect("conformance exists");

        assert_eq!(loaded_types.len(), 1);
        assert_eq!(loaded_envs.len(), 1);
        assert_eq!(loaded_transforms.len(), 1);
        assert_eq!(loaded_ports.len(), 1);
        assert_eq!(loaded_project_revision.id, project_revision.id);
        assert_eq!(
            loaded_project_revision.environments["python_sci"]["name"],
            "python_sci"
        );
        assert_eq!(
            loaded_project_revision.environments["python_sci"]["version"],
            "1"
        );
        assert_eq!(
            loaded_project_revision.transforms["clean_wp"]["environment"],
            "python_sci@1"
        );
        assert!(
            loaded_project_revision
                .endpoints
                .get("fetch_water_potential")
                .is_some()
        );
        assert_eq!(loaded_project_revision.project_meta["owner"], "user");
        assert_eq!(loaded_blob_artifact.artifact_kind, "blob");
        assert_eq!(
            loaded_blob_artifact.content_hash.as_deref(),
            Some("artifact_hash_1")
        );
        assert_eq!(
            decoded_manifest,
            ArtifactManifest::Bundle {
                entries: std::collections::BTreeMap::from([(
                    "raw".to_string(),
                    ozzy_core::artifacts::ArtifactManifestEntry {
                        artifact_id: blob_artifact.id
                    }
                )])
            }
        );
        assert_eq!(loaded_artifacts.len(), 2);
        assert_eq!(loaded_invocation_artifacts.len(), 2);
        assert_eq!(input_binding.binding_kind, "input");
        assert_eq!(output_binding.binding_kind, "output");
        assert_eq!(loaded_conformance.status, "declared");
    }

    #[tokio::test]
    async fn v4_project_revisions_reject_non_object_payloads() {
        let Some(db) = get_test_db().await else {
            return;
        };

        let user = db
            .upsert_user_from_github(
                rand::random::<i64>() & i64::MAX,
                &unique_name("v4user"),
                None,
                None,
            )
            .await
            .expect("create user");

        let project = db
            .create_project(user.id, &unique_name("v4proj"), Some("v4 test"), "private")
            .await
            .expect("create project");

        let commit = db
            .insert_commit(
                project.id,
                "github",
                "rileyleff/ozzydb",
                &format!("{:040x}", rand::random::<u64>()),
                "test_ozzy_toml_hash",
                user.id,
                Some("v4 registry test"),
            )
            .await
            .expect("insert commit");

        let registry_revision = db
            .insert_v4_registry_revision(project.id, user.id)
            .await
            .expect("insert registry revision");

        let err = db
            .insert_v4_project_revision(
                project.id,
                commit.id,
                registry_revision.id,
                "test_ozzy_toml_hash",
                "[project]\nname = \"test\"\nowner = \"user\"\n",
                json!(["not-an-object"]),
                json!({}),
                json!({}),
                json!({}),
                user.id,
            )
            .await
            .expect_err("array payload should violate JSON object checks");

        assert!(
            matches!(err, V4QueryError::Database(_)),
            "expected database constraint error, got {err:?}"
        );
    }

    #[tokio::test]
    async fn v4_manifest_artifacts_reject_cross_project_members() {
        let Some(db) = get_test_db().await else {
            return;
        };

        let user = db
            .upsert_user_from_github(
                rand::random::<i64>() & i64::MAX,
                &unique_name("v4user"),
                None,
                None,
            )
            .await
            .expect("create user");

        let project_a = db
            .create_project(user.id, &unique_name("v4proja"), Some("v4 test"), "private")
            .await
            .expect("create project a");
        let project_b = db
            .create_project(user.id, &unique_name("v4projb"), Some("v4 test"), "private")
            .await
            .expect("create project b");

        db.upsert_content_ref(
            "artifact_hash_cross_project",
            "content/ar/ti/artifact_hash_cross_project.bin",
            "application/octet-stream",
            42,
        )
        .await
        .expect("upsert content ref");

        let foreign_artifact = db
            .insert_v4_artifact(
                project_b.id,
                ArtifactKind::Blob,
                Some("artifact_hash_cross_project"),
                None,
                None,
                user.id,
            )
            .await
            .expect("insert foreign artifact");

        let err = db
            .insert_v4_manifest_artifact(
                project_a.id,
                &ArtifactManifest::Collection {
                    items: vec![ozzy_core::artifacts::ArtifactManifestEntry {
                        artifact_id: foreign_artifact.id,
                    }],
                },
                None,
                user.id,
            )
            .await
            .expect_err("cross-project manifest should fail");

        assert!(matches!(
            err,
            V4QueryError::CrossProjectArtifactReference { artifact_id, project_id }
            if artifact_id == foreign_artifact.id && project_id == project_a.id
        ));
    }
}
