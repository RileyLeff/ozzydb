//! Immutable v4 registry snapshots loaded from persisted registry revisions.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use uuid::Uuid;

use ozzy_types::canonical::{CanonicalType, CanonicalTypeId, CanonicalizationError};
use ozzy_types::ports::{TypedPort, TypedPortSet};
use ozzy_types::registry::{RegistryError, TypeRegistry, TypeVersion, TypeVersionId};
use ozzy_types::syntax::{TypeExpr, TypeRefExpr};

use crate::Database;
use crate::db::v4::{
    StoredCanonicalType, StoredEnvironmentVersion, StoredProjectRevision, StoredRegistryRevision,
    StoredTransformPort, StoredTransformVersion, StoredTypeVersion, V4QueryError,
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct VersionedName {
    pub name: String,
    pub version: String,
}

impl VersionedName {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SnapshotTransformVersion {
    pub row: StoredTransformVersion,
    pub inputs: TypedPortSet,
    pub outputs: TypedPortSet,
}

#[derive(Debug, Clone)]
pub struct RegistrySnapshot {
    revision: StoredRegistryRevision,
    type_registry: TypeRegistry,
    type_rows: BTreeMap<TypeVersionId, StoredTypeVersion>,
    type_version_ids_by_row_id: BTreeMap<Uuid, TypeVersionId>,
    canonical_types_by_row_id: BTreeMap<Uuid, CanonicalType>,
    canonical_row_ids_by_key: BTreeMap<CanonicalTypeId, Uuid>,
    equivalence_index: BTreeMap<CanonicalTypeId, Vec<TypeVersionId>>,
    environments: BTreeMap<VersionedName, StoredEnvironmentVersion>,
    environment_keys_by_row_id: BTreeMap<Uuid, VersionedName>,
    transforms: BTreeMap<VersionedName, SnapshotTransformVersion>,
    transform_keys_by_row_id: BTreeMap<Uuid, VersionedName>,
}

impl RegistrySnapshot {
    pub fn revision(&self) -> &StoredRegistryRevision {
        &self.revision
    }

    pub fn type_registry(&self) -> &TypeRegistry {
        &self.type_registry
    }

    pub fn type_row(&self, id: &TypeVersionId) -> Option<&StoredTypeVersion> {
        self.type_rows.get(id)
    }

    pub fn type_version_id_by_row_id(&self, row_id: Uuid) -> Option<&TypeVersionId> {
        self.type_version_ids_by_row_id.get(&row_id)
    }

    pub fn resolve_type_ref(
        &self,
        type_ref: &TypeRefExpr,
    ) -> Result<(&TypeVersion, &StoredTypeVersion), RegistryError> {
        let type_version = self.type_registry.resolve_ref(type_ref)?;
        let row = self.type_rows.get(&type_version.id).ok_or_else(|| {
            RegistryError::UnknownTypeVersionReference {
                name: type_ref.name.clone(),
                version: type_ref
                    .version
                    .clone()
                    .unwrap_or_else(|| "<unversioned>".to_string()),
            }
        })?;
        Ok((type_version, row))
    }

    pub fn canonical_type_by_row_id(&self, row_id: Uuid) -> Option<&CanonicalType> {
        self.canonical_types_by_row_id.get(&row_id)
    }

    pub fn canonical_row_id_by_key(&self, key: &CanonicalTypeId) -> Option<Uuid> {
        self.canonical_row_ids_by_key.get(key).copied()
    }

    pub fn equivalent_type_versions(&self, id: &TypeVersionId) -> Option<&[TypeVersionId]> {
        let canonical = self.type_registry.get(id)?.canonical.as_ref()?;
        self.equivalence_index.get(canonical).map(Vec::as_slice)
    }

    pub fn environment(&self, name: &str, version: &str) -> Option<&StoredEnvironmentVersion> {
        self.environments
            .get(&VersionedName::new(name.to_string(), version.to_string()))
    }

    pub fn environment_by_row_id(&self, row_id: Uuid) -> Option<&StoredEnvironmentVersion> {
        let key = self.environment_keys_by_row_id.get(&row_id)?;
        self.environments.get(key)
    }

    pub fn transform(&self, name: &str, version: &str) -> Option<&SnapshotTransformVersion> {
        self.transforms
            .get(&VersionedName::new(name.to_string(), version.to_string()))
    }

    pub fn transform_by_row_id(&self, row_id: Uuid) -> Option<&SnapshotTransformVersion> {
        let key = self.transform_keys_by_row_id.get(&row_id)?;
        self.transforms.get(key)
    }
}

#[derive(Debug, Clone, Default)]
pub struct RegistrySnapshotCache {
    inner: Arc<RwLock<BTreeMap<Uuid, Arc<RegistrySnapshot>>>>,
}

impl RegistrySnapshotCache {
    pub async fn get(&self, registry_revision_id: Uuid) -> Option<Arc<RegistrySnapshot>> {
        let guard = self.inner.read().await;
        guard.get(&registry_revision_id).cloned()
    }

    pub async fn insert(
        &self,
        registry_revision_id: Uuid,
        snapshot: Arc<RegistrySnapshot>,
    ) -> Arc<RegistrySnapshot> {
        let mut guard = self.inner.write().await;
        guard
            .entry(registry_revision_id)
            .or_insert_with(|| snapshot.clone())
            .clone()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RegistrySnapshotError {
    #[error(transparent)]
    Query(#[from] V4QueryError),
    #[error(transparent)]
    Registry(#[from] RegistryError),
    #[error(transparent)]
    Canonicalization(#[from] CanonicalizationError),
    #[error("failed to deserialize persisted v4 object: {0}")]
    Deserialization(#[from] serde_json::Error),
    #[error("registry revision '{registry_revision_id}' does not exist")]
    UnknownRegistryRevision { registry_revision_id: Uuid },
    #[error("project revision for commit '{commit_id}' does not exist")]
    UnknownProjectRevisionForCommit { commit_id: Uuid },
    #[error(
        "type version '{type_name}@{type_version}' points at missing canonical type row '{canonical_type_row_id}'"
    )]
    MissingCanonicalTypeForVersion {
        type_name: String,
        type_version: String,
        canonical_type_row_id: Uuid,
    },
    #[error(
        "stored canonical type row '{canonical_type_row_id}' has key '{stored_key}' but canonicalizes to '{computed_key}'"
    )]
    CanonicalKeyMismatch {
        canonical_type_row_id: Uuid,
        stored_key: String,
        computed_key: String,
    },
    #[error(
        "transform '{transform_name}@{transform_version}' points at missing environment row '{environment_row_id}' in the snapshot"
    )]
    MissingEnvironmentForTransform {
        transform_name: String,
        transform_version: String,
        environment_row_id: Uuid,
    },
    #[error("transform '{transform_name}@{transform_version}' has unknown port kind '{port_kind}'")]
    UnknownTransformPortKind {
        transform_name: String,
        transform_version: String,
        port_kind: String,
    },
    #[error(
        "transform '{transform_name}@{transform_version}' port '{port_kind}/{port_name}' points at missing type row '{type_row_id}'"
    )]
    MissingTypeForTransformPort {
        transform_name: String,
        transform_version: String,
        port_kind: String,
        port_name: String,
        type_row_id: Uuid,
    },
    #[error(
        "transform '{transform_name}@{transform_version}' defines duplicate port '{port_kind}/{port_name}'"
    )]
    DuplicateTransformPort {
        transform_name: String,
        transform_version: String,
        port_kind: String,
        port_name: String,
    },
}

pub async fn load_registry_snapshot(
    db: &Database,
    cache: &RegistrySnapshotCache,
    registry_revision_id: Uuid,
) -> Result<Arc<RegistrySnapshot>, RegistrySnapshotError> {
    if let Some(snapshot) = cache.get(registry_revision_id).await {
        return Ok(snapshot);
    }

    let loaded = Arc::new(load_registry_snapshot_uncached(db, registry_revision_id).await?);
    Ok(cache.insert(registry_revision_id, loaded).await)
}

pub async fn load_project_revision_snapshot_by_commit(
    db: &Database,
    cache: &RegistrySnapshotCache,
    commit_id: Uuid,
) -> Result<(StoredProjectRevision, Arc<RegistrySnapshot>), RegistrySnapshotError> {
    let project_revision = db
        .get_v4_project_revision_by_commit(commit_id)
        .await?
        .ok_or(RegistrySnapshotError::UnknownProjectRevisionForCommit { commit_id })?;
    let snapshot = load_registry_snapshot(db, cache, project_revision.registry_revision_id).await?;
    Ok((project_revision, snapshot))
}

async fn load_registry_snapshot_uncached(
    db: &Database,
    registry_revision_id: Uuid,
) -> Result<RegistrySnapshot, RegistrySnapshotError> {
    let revision = db
        .get_v4_registry_revision(registry_revision_id)
        .await?
        .ok_or(RegistrySnapshotError::UnknownRegistryRevision {
            registry_revision_id,
        })?;

    let stored_canonical_types = db
        .list_v4_registry_revision_canonical_types(registry_revision_id)
        .await?;
    let stored_type_versions = db
        .list_v4_registry_revision_type_versions(registry_revision_id)
        .await?;
    let stored_environment_versions = db
        .list_v4_registry_revision_environment_versions(registry_revision_id)
        .await?;
    let stored_transform_versions = db
        .list_v4_registry_revision_transform_versions(registry_revision_id)
        .await?;
    let stored_transform_ports = db
        .list_v4_registry_revision_transform_ports(registry_revision_id)
        .await?;

    let mut canonical_types_by_row_id = BTreeMap::new();
    let mut canonical_row_ids_by_key = BTreeMap::new();

    for stored in stored_canonical_types {
        let canonical = decode_canonical_type(&stored)?;
        canonical_row_ids_by_key.insert(canonical.id.clone(), stored.id);
        canonical_types_by_row_id.insert(stored.id, canonical);
    }

    let mut type_registry = TypeRegistry::default();
    let mut type_rows = BTreeMap::new();
    let mut type_version_ids_by_row_id = BTreeMap::new();
    let mut equivalence_index = BTreeMap::new();

    for stored in stored_type_versions {
        let canonical = canonical_types_by_row_id
            .get(&stored.canonical_type_id)
            .ok_or(RegistrySnapshotError::MissingCanonicalTypeForVersion {
                type_name: stored.name.clone(),
                type_version: stored.version.clone(),
                canonical_type_row_id: stored.canonical_type_id,
            })?;

        let expr: TypeExpr = serde_json::from_value(stored.expr.clone())?;
        let mut type_version = TypeVersion::new(stored.name.clone(), stored.version.clone(), expr);
        type_version.canonical = Some(canonical.id.clone());

        type_registry.insert(type_version.clone())?;
        equivalence_index
            .entry(canonical.id.clone())
            .or_insert_with(Vec::new)
            .push(type_version.id.clone());
        type_version_ids_by_row_id.insert(stored.id, type_version.id.clone());
        type_rows.insert(type_version.id, stored);
    }

    let mut environments = BTreeMap::new();
    let mut environment_keys_by_row_id = BTreeMap::new();
    for stored in stored_environment_versions {
        let key = VersionedName::new(stored.name.clone(), stored.version.clone());
        environment_keys_by_row_id.insert(stored.id, key.clone());
        environments.insert(key, stored);
    }

    let mut ports_by_transform = BTreeMap::<Uuid, Vec<StoredTransformPort>>::new();
    for port in stored_transform_ports {
        ports_by_transform
            .entry(port.transform_version_id)
            .or_default()
            .push(port);
    }

    let mut transforms = BTreeMap::new();
    let mut transform_keys_by_row_id = BTreeMap::new();
    for stored in stored_transform_versions {
        if !environment_keys_by_row_id.contains_key(&stored.environment_version_id) {
            return Err(RegistrySnapshotError::MissingEnvironmentForTransform {
                transform_name: stored.name.clone(),
                transform_version: stored.version.clone(),
                environment_row_id: stored.environment_version_id,
            });
        }

        let mut inputs = TypedPortSet::default();
        let mut outputs = TypedPortSet::default();
        for port in ports_by_transform.remove(&stored.id).unwrap_or_default() {
            let Some(type_version_id) = type_version_ids_by_row_id.get(&port.type_version_id)
            else {
                return Err(RegistrySnapshotError::MissingTypeForTransformPort {
                    transform_name: stored.name.clone(),
                    transform_version: stored.version.clone(),
                    port_kind: port.port_kind.clone(),
                    port_name: port.port_name.clone(),
                    type_row_id: port.type_version_id,
                });
            };
            let Some(type_row) = type_rows.get(type_version_id) else {
                return Err(RegistrySnapshotError::MissingTypeForTransformPort {
                    transform_name: stored.name.clone(),
                    transform_version: stored.version.clone(),
                    port_kind: port.port_kind.clone(),
                    port_name: port.port_name.clone(),
                    type_row_id: port.type_version_id,
                });
            };

            let typed_port = TypedPort {
                ty: TypeRefExpr::new(type_row.name.clone(), Some(type_row.version.clone())),
                description: port.description.clone(),
            };

            let target_set = match port.port_kind.as_str() {
                "input" => &mut inputs,
                "output" => &mut outputs,
                other => {
                    return Err(RegistrySnapshotError::UnknownTransformPortKind {
                        transform_name: stored.name.clone(),
                        transform_version: stored.version.clone(),
                        port_kind: other.to_string(),
                    });
                }
            };

            if target_set
                .insert(port.port_name.clone(), typed_port)
                .is_some()
            {
                return Err(RegistrySnapshotError::DuplicateTransformPort {
                    transform_name: stored.name.clone(),
                    transform_version: stored.version.clone(),
                    port_kind: port.port_kind.clone(),
                    port_name: port.port_name,
                });
            }
        }

        let key = VersionedName::new(stored.name.clone(), stored.version.clone());
        transform_keys_by_row_id.insert(stored.id, key.clone());
        transforms.insert(
            key,
            SnapshotTransformVersion {
                row: stored,
                inputs,
                outputs,
            },
        );
    }

    Ok(RegistrySnapshot {
        revision,
        type_registry,
        type_rows,
        type_version_ids_by_row_id,
        canonical_types_by_row_id,
        canonical_row_ids_by_key,
        equivalence_index,
        environments,
        environment_keys_by_row_id,
        transforms,
        transform_keys_by_row_id,
    })
}

fn decode_canonical_type(
    stored: &StoredCanonicalType,
) -> Result<CanonicalType, RegistrySnapshotError> {
    let expr: TypeExpr = serde_json::from_value(stored.expr.clone())?;
    let canonical = CanonicalType::new(expr)?;
    if canonical.id.as_str() != stored.canonical_key {
        return Err(RegistrySnapshotError::CanonicalKeyMismatch {
            canonical_type_row_id: stored.id,
            stored_key: stored.canonical_key.clone(),
            computed_key: canonical.id.as_str().to_string(),
        });
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use sqlx::postgres::PgPoolOptions;
    use std::env;

    use ozzy_types::canonical::CanonicalType;
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
    async fn registry_snapshots_load_and_cache_pinned_revisions() {
        let Some(db) = get_test_db().await else {
            return;
        };

        let user = db
            .upsert_user_from_github(
                rand::random::<i64>() & i64::MAX,
                &unique_name("snapuser"),
                None,
                None,
            )
            .await
            .expect("create user");
        let project = db
            .create_project(
                user.id,
                &unique_name("snapproj"),
                Some("snapshot test"),
                "private",
            )
            .await
            .expect("create project");
        let commit = db
            .insert_commit(
                project.id,
                "github",
                "rileyleff/ozzydb",
                &format!("{:040x}", rand::random::<u64>()),
                "snapshot_ozzy_toml_hash",
                user.id,
                Some("registry snapshot test"),
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
                json!({ "base": "python", "lockfile": "uv.lock" }),
            )
            .await
            .expect("insert environment");
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
            .expect("insert transform");
        db.insert_v4_transform_port(
            stored_transform.id,
            crate::db::v4::TransformPortKind::Input,
            "raw",
            stored_type.id,
            Some("raw input"),
        )
        .await
        .expect("insert transform port");
        db.insert_v4_transform_port(
            stored_transform.id,
            crate::db::v4::TransformPortKind::Output,
            "result",
            stored_type.id,
            Some("cleaned output"),
        )
        .await
        .expect("insert transform port");

        let revision = db
            .insert_v4_registry_revision(project.id, user.id)
            .await
            .expect("insert registry revision");
        db.attach_v4_type_version_to_registry_revision(revision.id, stored_type.id)
            .await
            .expect("attach type");
        db.attach_v4_environment_version_to_registry_revision(revision.id, stored_env.id)
            .await
            .expect("attach env");
        db.attach_v4_transform_version_to_registry_revision(revision.id, stored_transform.id)
            .await
            .expect("attach transform");
        let project_revision = db
            .insert_v4_project_revision(
                project.id,
                commit.id,
                revision.id,
                "snapshot_ozzy_toml_hash",
                "[types]\nWaterPotential = 'float64'\n",
                user.id,
            )
            .await
            .expect("insert project revision");

        let cache = RegistrySnapshotCache::default();
        let snapshot = load_registry_snapshot(&db, &cache, revision.id)
            .await
            .expect("load snapshot");
        let snapshot_again = load_registry_snapshot(&db, &cache, revision.id)
            .await
            .expect("load cached snapshot");
        let (loaded_project_revision, loaded_by_commit) =
            load_project_revision_snapshot_by_commit(&db, &cache, commit.id)
                .await
                .expect("load snapshot by commit");

        assert!(Arc::ptr_eq(&snapshot, &snapshot_again));
        assert!(Arc::ptr_eq(&snapshot, &loaded_by_commit));
        assert_eq!(loaded_project_revision.id, project_revision.id);
        assert_eq!(snapshot.revision().id, revision.id);

        let (resolved_type, resolved_row) = snapshot
            .resolve_type_ref(&TypeRefExpr::new(
                "std/WaterPotential",
                Some("1".to_string()),
            ))
            .expect("resolve type ref");
        assert_eq!(resolved_type.name, "std/WaterPotential");
        assert_eq!(resolved_row.id, stored_type.id);
        assert_eq!(
            snapshot
                .equivalent_type_versions(&resolved_type.id)
                .expect("equivalence class")
                .len(),
            1
        );

        let transform = snapshot
            .transform("clean_wp", "1")
            .expect("transform should exist");
        assert!(transform.inputs.ports.contains_key("raw"));
        assert!(transform.outputs.ports.contains_key("result"));

        let environment = snapshot
            .environment("python_sci", "1")
            .expect("environment should exist");
        assert_eq!(environment.id, stored_env.id);
    }
}
