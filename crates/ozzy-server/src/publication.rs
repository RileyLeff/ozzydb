//! Atomic v4 publication pipeline.
//!
//! Push compiles authored `ozzy.toml` into a `PublicationBundle`, resolves or
//! creates versioned registry objects, and publishes a new project revision in
//! one database transaction.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;
use sqlx::{Postgres, Transaction};
use thiserror::Error;
use uuid::Uuid;

use ozzy_core::toml_spec::{OzzyToml, PublishedEnvironmentDef, TransformDef};
use ozzy_types::canonical::{CanonicalType, CanonicalizationError};
use ozzy_types::ports::{TypedPort, TypedPortSet};
use ozzy_types::registry::TypeVersion;
use ozzy_types::syntax::{BuiltinType, TypeDefinitions, TypeExpr, TypeRefExpr};

use crate::Database;
use crate::db::models::{Commit, Ref};
use crate::db::v4::{
    StoredCanonicalType, StoredEnvironmentVersion, StoredProjectRevision, StoredTransformPort,
    StoredTransformVersion, StoredTypeVersion,
};
use crate::registry::VersionedName;

#[derive(Debug)]
pub struct PublicationBundle {
    pub environments_payload: Value,
    pub transforms_payload: Value,
    pub endpoints_payload: Value,
    pub project_meta_payload: Value,
    pub types: BTreeMap<String, StoredTypeVersion>,
    pub environments: BTreeMap<String, StoredEnvironmentVersion>,
    pub transforms: BTreeMap<String, StoredTransformVersion>,
}

#[derive(Debug)]
pub enum PublishOutcome {
    Created {
        commit: Commit,
        bundle: PublicationBundle,
    },
    Existing {
        commit: Commit,
    },
}

#[derive(Debug, Error)]
pub enum PublicationError {
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Serialization(#[from] serde_json::Error),
    #[error(transparent)]
    Canonicalization(#[from] CanonicalizationError),
    #[error("published type '{name}@{version}' does not exist in project '{project_id}'")]
    MissingPublishedTypeReference {
        project_id: Uuid,
        name: String,
        version: String,
    },
    #[error(
        "published type '{name}@{version}' is missing canonical type row '{canonical_type_id}'"
    )]
    MissingCanonicalTypeForPublishedReference {
        name: String,
        version: String,
        canonical_type_id: Uuid,
    },
    #[error("recursive local type definition detected during publication: {cycle:?}")]
    RecursiveLocalTypeDefinition { cycle: Vec<String> },
    #[error("type definition '{name}' could not be resolved during publication")]
    UnknownLocalTypeDefinition { name: String },
    #[error(
        "auto-versioning requires numeric versions for '{entity}' named '{name}', found '{version}'"
    )]
    NonNumericVersion {
        entity: &'static str,
        name: String,
        version: String,
    },
    #[error(
        "transform port '{transform_name}.{port_name}' references unknown local type '{type_name}'"
    )]
    UnknownTransformPortType {
        transform_name: String,
        port_name: String,
        type_name: String,
    },
    #[error(
        "transform port '{transform_name}.{port_name}' uses unsupported builtin type '{type_name}' directly; define it in [types] first"
    )]
    BuiltinPortTypeMustBeNamed {
        transform_name: String,
        port_name: String,
        type_name: String,
    },
    #[error(
        "concurrent duplicate commit '{git_commit_sha}' exists without a published v4 project revision"
    )]
    ExistingCommitMissingProjectRevision { git_commit_sha: String },
    #[error(
        "transform '{transform_name}' references missing authored environment '{environment_name}'"
    )]
    UnknownTransformEnvironment {
        transform_name: String,
        environment_name: String,
    },
}

#[derive(Debug, Clone)]
pub struct PublishCommitInput<'a> {
    pub project_id: Uuid,
    pub pushed_by: Uuid,
    pub git_provider: &'a str,
    pub git_repo: &'a str,
    pub git_commit_sha: &'a str,
    pub ozzy_toml_hash: &'a str,
    pub message: Option<&'a str>,
    pub ref_name: Option<&'a str>,
    pub ozzy_toml_raw: &'a str,
    pub ozzy_toml: &'a OzzyToml,
    pub published_environments: &'a BTreeMap<String, PublishedEnvironmentDef>,
}

pub async fn publish_v4_commit_atomically(
    db: &Database,
    input: PublishCommitInput<'_>,
) -> Result<PublishOutcome, PublicationError> {
    let mut tx = db.pool().begin().await?;

    sqlx::query("SELECT id FROM projects WHERE id = $1 FOR UPDATE")
        .bind(input.project_id)
        .execute(&mut *tx)
        .await?;

    if let Some(existing_commit) =
        get_commit_by_sha_tx(&mut tx, input.project_id, input.git_commit_sha).await?
    {
        let existing_revision =
            get_project_revision_by_commit_tx(&mut tx, existing_commit.id).await?;
        if existing_revision.is_none() {
            return Err(PublicationError::ExistingCommitMissingProjectRevision {
                git_commit_sha: input.git_commit_sha.to_string(),
            });
        }

        if let Some(ref_name) = input.ref_name {
            upsert_ref_tx(
                &mut tx,
                input.project_id,
                ref_name,
                "branch",
                existing_commit.id,
            )
            .await?;
        }

        tx.commit().await?;
        return Ok(PublishOutcome::Existing {
            commit: existing_commit,
        });
    }

    let commit = insert_commit_tx(
        &mut tx,
        input.project_id,
        input.git_provider,
        input.git_repo,
        input.git_commit_sha,
        input.ozzy_toml_hash,
        input.pushed_by,
        input.message,
    )
    .await?;

    let bundle = compile_publication_bundle(
        &mut tx,
        input.project_id,
        input.pushed_by,
        input.ozzy_toml,
        input.published_environments,
    )
    .await?;

    let registry_revision =
        insert_registry_revision_tx(&mut tx, input.project_id, input.pushed_by).await?;

    for type_row in bundle.types.values() {
        attach_type_version_tx(&mut tx, registry_revision.id, type_row.id).await?;
    }
    for env_row in bundle.environments.values() {
        attach_environment_version_tx(&mut tx, registry_revision.id, env_row.id).await?;
    }
    for transform_row in bundle.transforms.values() {
        attach_transform_version_tx(&mut tx, registry_revision.id, transform_row.id).await?;
    }

    insert_project_revision_tx(
        &mut tx,
        input.project_id,
        commit.id,
        registry_revision.id,
        input.ozzy_toml_hash,
        input.ozzy_toml_raw,
        &bundle.environments_payload,
        &bundle.transforms_payload,
        &bundle.endpoints_payload,
        &bundle.project_meta_payload,
        input.pushed_by,
    )
    .await?;

    if let Some(ref_name) = input.ref_name {
        upsert_ref_tx(&mut tx, input.project_id, ref_name, "branch", commit.id).await?;
    }

    tx.commit().await?;

    Ok(PublishOutcome::Created { commit, bundle })
}

async fn compile_publication_bundle(
    tx: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    published_by: Uuid,
    ozzy_toml: &OzzyToml,
    published_environments: &BTreeMap<String, PublishedEnvironmentDef>,
) -> Result<PublicationBundle, PublicationError> {
    let local_types =
        resolve_or_publish_types(tx, project_id, published_by, &ozzy_toml.types).await?;
    let environments =
        resolve_or_publish_environments(tx, project_id, published_by, published_environments)
            .await?;
    let (transforms, rewritten_transforms, referenced_external_types) =
        resolve_or_publish_transforms(
            tx,
            project_id,
            published_by,
            &ozzy_toml.transforms,
            &local_types,
            &environments,
        )
        .await?;

    let mut attached_types = local_types.clone();
    for (key, row) in referenced_external_types {
        attached_types.entry(key).or_insert(row);
    }

    Ok(PublicationBundle {
        environments_payload: build_environment_bindings_payload(&environments)?,
        transforms_payload: serde_json::to_value(&rewritten_transforms)?,
        endpoints_payload: serde_json::to_value(&ozzy_toml.endpoints)?,
        project_meta_payload: serde_json::to_value(&ozzy_toml.project)?,
        types: attached_types,
        environments,
        transforms,
    })
}

async fn resolve_or_publish_types(
    tx: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    published_by: Uuid,
    definitions: &TypeDefinitions,
) -> Result<BTreeMap<String, StoredTypeVersion>, PublicationError> {
    let external_refs = collect_external_type_refs(definitions)?;
    let external_canonical_exprs =
        load_external_canonical_exprs(tx, project_id, &external_refs).await?;

    let mut published = BTreeMap::new();
    for (name, definition) in &definitions.types {
        let expanded = expand_local_type_expr(
            definitions,
            &external_canonical_exprs,
            &definition.expr,
            &mut Vec::new(),
        )?;
        let canonical = CanonicalType::canonicalize(&TypeDefinitions::default(), &expanded)?;
        let canonical_row = resolve_or_insert_canonical_type(tx, &canonical).await?;

        let existing_versions = list_type_versions_by_name_tx(tx, project_id, name).await?;
        let row = if let Some(existing) = existing_versions
            .iter()
            .find(|row| row.canonical_type_id == canonical_row.id)
        {
            existing.clone()
        } else {
            let version = next_numeric_version(
                "type version",
                name,
                existing_versions.iter().map(|row| row.version.as_str()),
            )?;
            insert_type_version_tx(
                tx,
                project_id,
                published_by,
                &TypeVersion::new(name.clone(), version, canonical.expr.clone()),
                canonical_row.id,
            )
            .await?
        };

        published.insert(name.clone(), row);
    }

    Ok(published)
}

async fn resolve_or_publish_environments(
    tx: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    published_by: Uuid,
    authored: &BTreeMap<String, PublishedEnvironmentDef>,
) -> Result<BTreeMap<String, StoredEnvironmentVersion>, PublicationError> {
    let mut published = BTreeMap::new();

    for (name, definition) in authored {
        let definition_value = serde_json::to_value(definition)?;
        let existing_versions = list_environment_versions_by_name_tx(tx, project_id, name).await?;

        let row = if let Some(existing) = existing_versions
            .iter()
            .find(|row| row.definition == definition_value)
        {
            existing.clone()
        } else {
            let version = next_numeric_version(
                "environment version",
                name,
                existing_versions.iter().map(|row| row.version.as_str()),
            )?;
            insert_environment_version_tx(
                tx,
                project_id,
                published_by,
                name,
                &version,
                &definition_value,
            )
            .await?
        };

        published.insert(name.clone(), row);
    }

    Ok(published)
}

fn build_environment_bindings_payload(
    environments: &BTreeMap<String, StoredEnvironmentVersion>,
) -> Result<Value, serde_json::Error> {
    let bindings = environments
        .iter()
        .map(|(authored_name, row)| {
            (
                authored_name.clone(),
                VersionedName::new(row.name.clone(), row.version.clone()),
            )
        })
        .collect::<BTreeMap<_, _>>();
    serde_json::to_value(bindings)
}

type ExternalTypeRowMap = BTreeMap<String, StoredTypeVersion>;

async fn resolve_or_publish_transforms(
    tx: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    published_by: Uuid,
    authored: &std::collections::HashMap<String, TransformDef>,
    local_types: &BTreeMap<String, StoredTypeVersion>,
    environments: &BTreeMap<String, StoredEnvironmentVersion>,
) -> Result<
    (
        BTreeMap<String, StoredTransformVersion>,
        BTreeMap<String, TransformDef>,
        ExternalTypeRowMap,
    ),
    PublicationError,
> {
    let mut published = BTreeMap::new();
    let mut rewritten = BTreeMap::new();
    let mut external_types = BTreeMap::new();

    for (name, transform) in authored {
        let environment = environments.get(&transform.environment).ok_or_else(|| {
            PublicationError::UnknownTransformEnvironment {
                transform_name: name.clone(),
                environment_name: transform.environment.clone(),
            }
        })?;

        let (input_rows, rewritten_inputs, input_external) =
            resolve_transform_ports(tx, project_id, name, &transform.inputs, local_types).await?;
        let (output_rows, rewritten_outputs, output_external) =
            resolve_transform_ports(tx, project_id, name, &transform.outputs, local_types).await?;

        external_types.extend(input_external);
        external_types.extend(output_external);

        let params_schema = serde_json::to_value(&transform.params)?;
        let existing_versions = list_transform_versions_by_name_tx(tx, project_id, name).await?;

        let existing_match = find_matching_transform_version(
            tx,
            &existing_versions,
            environment.id,
            transform,
            &input_rows,
            &output_rows,
        )
        .await?;

        let row = if let Some(existing) = existing_match {
            existing
        } else {
            let version = next_numeric_version(
                "transform version",
                name,
                existing_versions.iter().map(|row| row.version.as_str()),
            )?;
            let inserted = insert_transform_version_tx(
                tx,
                project_id,
                published_by,
                name,
                &version,
                environment.id,
                transform,
                &params_schema,
            )
            .await?;

            insert_transform_ports_tx(
                tx,
                inserted.id,
                &input_rows,
                &output_rows,
                &transform.inputs,
                &transform.outputs,
            )
            .await?;
            inserted
        };

        let rewritten_transform = TransformDef {
            source: transform.source.clone(),
            command: transform.command.clone(),
            environment: format!("{}@{}", environment.name, environment.version),
            description: transform.description.clone(),
            inputs: rewritten_inputs,
            outputs: rewritten_outputs,
            params: transform.params.clone(),
            network: transform.network,
            secrets: transform.secrets.clone(),
        };

        published.insert(name.clone(), row);
        rewritten.insert(name.clone(), rewritten_transform);
    }

    Ok((published, rewritten, external_types))
}

async fn find_matching_transform_version(
    tx: &mut Transaction<'_, Postgres>,
    candidates: &[StoredTransformVersion],
    expected_environment_version_id: Uuid,
    authored: &TransformDef,
    input_rows: &BTreeMap<String, StoredTypeVersion>,
    output_rows: &BTreeMap<String, StoredTypeVersion>,
) -> Result<Option<StoredTransformVersion>, PublicationError> {
    let expected_input_ids = input_rows
        .iter()
        .map(|(name, row)| (name.clone(), row.id))
        .collect::<BTreeMap<_, _>>();
    let expected_output_ids = output_rows
        .iter()
        .map(|(name, row)| (name.clone(), row.id))
        .collect::<BTreeMap<_, _>>();

    for candidate in candidates {
        if candidate.environment_version_id != expected_environment_version_id
            || candidate.source_ref != authored.source
            || candidate.command != authored.command
            || candidate.description != authored.description
            || candidate.params_schema != serde_json::to_value(&authored.params)?
            || candidate.network_access != authored.network
            || candidate.secrets != authored.secrets
        {
            continue;
        }

        let ports = list_transform_ports_tx(tx, candidate.id).await?;
        let candidate_inputs = ports
            .iter()
            .filter(|port| port.port_kind == "input")
            .map(|port| (port.port_name.clone(), port.type_version_id))
            .collect::<BTreeMap<_, _>>();
        let candidate_outputs = ports
            .iter()
            .filter(|port| port.port_kind == "output")
            .map(|port| (port.port_name.clone(), port.type_version_id))
            .collect::<BTreeMap<_, _>>();

        if candidate_inputs == expected_input_ids && candidate_outputs == expected_output_ids {
            return Ok(Some(candidate.clone()));
        }
    }

    Ok(None)
}

async fn resolve_transform_ports(
    tx: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    transform_name: &str,
    authored_ports: &TypedPortSet,
    local_types: &BTreeMap<String, StoredTypeVersion>,
) -> Result<
    (
        BTreeMap<String, StoredTypeVersion>,
        TypedPortSet,
        ExternalTypeRowMap,
    ),
    PublicationError,
> {
    let mut rows = BTreeMap::new();
    let mut rewritten = TypedPortSet::default();
    let mut external = BTreeMap::new();

    for (port_name, port) in &authored_ports.ports {
        let row = match &port.ty.version {
            Some(version) => {
                let stored = get_type_version_tx(tx, project_id, &port.ty.name, version)
                    .await?
                    .ok_or_else(|| PublicationError::MissingPublishedTypeReference {
                        project_id,
                        name: port.ty.name.clone(),
                        version: version.clone(),
                    })?;
                external.insert(
                    format!("{}@{}", stored.name, stored.version),
                    stored.clone(),
                );
                stored
            }
            None => {
                if BuiltinType::parse(&port.ty.name).is_some() {
                    return Err(PublicationError::BuiltinPortTypeMustBeNamed {
                        transform_name: transform_name.to_string(),
                        port_name: port_name.clone(),
                        type_name: port.ty.name.clone(),
                    });
                }
                local_types.get(&port.ty.name).cloned().ok_or_else(|| {
                    PublicationError::UnknownTransformPortType {
                        transform_name: transform_name.to_string(),
                        port_name: port_name.clone(),
                        type_name: port.ty.name.clone(),
                    }
                })?
            }
        };

        rows.insert(port_name.clone(), row.clone());
        rewritten.insert(
            port_name.clone(),
            TypedPort {
                ty: TypeRefExpr::new(row.name.clone(), Some(row.version.clone())),
                description: port.description.clone(),
            },
        );
    }

    Ok((rows, rewritten, external))
}

fn collect_external_type_refs(
    definitions: &TypeDefinitions,
) -> Result<BTreeSet<(String, String)>, PublicationError> {
    let mut refs = BTreeSet::new();
    for definition in definitions.types.values() {
        collect_external_type_refs_in_expr(
            definitions,
            &definition.expr,
            &mut refs,
            &mut Vec::new(),
        )?;
    }
    Ok(refs)
}

fn collect_external_type_refs_in_expr(
    definitions: &TypeDefinitions,
    expr: &TypeExpr,
    refs: &mut BTreeSet<(String, String)>,
    stack: &mut Vec<String>,
) -> Result<(), PublicationError> {
    match expr {
        TypeExpr::Builtin(_) | TypeExpr::Never => Ok(()),
        TypeExpr::Ref(type_ref) => {
            if BuiltinType::parse(&type_ref.name).is_some() {
                return Ok(());
            }
            if let Some(version) = &type_ref.version {
                refs.insert((type_ref.name.clone(), version.clone()));
                return Ok(());
            }
            if stack.contains(&type_ref.name) {
                let mut cycle = stack.clone();
                cycle.push(type_ref.name.clone());
                return Err(PublicationError::RecursiveLocalTypeDefinition { cycle });
            }
            let definition = definitions.get(&type_ref.name).ok_or_else(|| {
                PublicationError::UnknownLocalTypeDefinition {
                    name: type_ref.name.clone(),
                }
            })?;
            stack.push(type_ref.name.clone());
            let result =
                collect_external_type_refs_in_expr(definitions, &definition.expr, refs, stack);
            stack.pop();
            result
        }
        TypeExpr::Intersection(parts) => {
            for part in parts {
                collect_external_type_refs_in_expr(definitions, part, refs, stack)?;
            }
            Ok(())
        }
        TypeExpr::Constructor(_) => Ok(()),
        TypeExpr::Record(record) => {
            for field in &record.fields {
                collect_external_type_refs_in_expr(definitions, &field.ty, refs, stack)?;
            }
            Ok(())
        }
        TypeExpr::Collection(item) | TypeExpr::Table(item) => {
            collect_external_type_refs_in_expr(definitions, item, refs, stack)
        }
    }
}

async fn load_external_canonical_exprs(
    tx: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    refs: &BTreeSet<(String, String)>,
) -> Result<BTreeMap<(String, String), TypeExpr>, PublicationError> {
    let mut loaded = BTreeMap::new();

    for (name, version) in refs {
        let stored = get_type_version_tx(tx, project_id, name, version)
            .await?
            .ok_or_else(|| PublicationError::MissingPublishedTypeReference {
                project_id,
                name: name.clone(),
                version: version.clone(),
            })?;
        let canonical = get_canonical_type_by_id_tx(tx, stored.canonical_type_id)
            .await?
            .ok_or_else(
                || PublicationError::MissingCanonicalTypeForPublishedReference {
                    name: name.clone(),
                    version: version.clone(),
                    canonical_type_id: stored.canonical_type_id,
                },
            )?;
        let expr = serde_json::from_value(canonical.expr)?;
        loaded.insert((name.clone(), version.clone()), expr);
    }

    Ok(loaded)
}

fn expand_local_type_expr(
    definitions: &TypeDefinitions,
    external_canonical_exprs: &BTreeMap<(String, String), TypeExpr>,
    expr: &TypeExpr,
    stack: &mut Vec<String>,
) -> Result<TypeExpr, PublicationError> {
    match expr {
        TypeExpr::Builtin(_) | TypeExpr::Never => Ok(expr.clone()),
        TypeExpr::Ref(type_ref) => {
            if let Some(builtin) = BuiltinType::parse(&type_ref.name) {
                return Ok(TypeExpr::Builtin(builtin));
            }

            if let Some(version) = &type_ref.version {
                return external_canonical_exprs
                    .get(&(type_ref.name.clone(), version.clone()))
                    .cloned()
                    .ok_or_else(|| PublicationError::MissingPublishedTypeReference {
                        project_id: Uuid::nil(),
                        name: type_ref.name.clone(),
                        version: version.clone(),
                    });
            }

            if stack.contains(&type_ref.name) {
                let mut cycle = stack.clone();
                cycle.push(type_ref.name.clone());
                return Err(PublicationError::RecursiveLocalTypeDefinition { cycle });
            }

            let definition = definitions.get(&type_ref.name).ok_or_else(|| {
                PublicationError::UnknownLocalTypeDefinition {
                    name: type_ref.name.clone(),
                }
            })?;
            stack.push(type_ref.name.clone());
            let result = expand_local_type_expr(
                definitions,
                external_canonical_exprs,
                &definition.expr,
                stack,
            );
            stack.pop();
            result
        }
        TypeExpr::Intersection(parts) => Ok(TypeExpr::Intersection(
            parts
                .iter()
                .map(|part| {
                    expand_local_type_expr(definitions, external_canonical_exprs, part, stack)
                })
                .collect::<Result<Vec<_>, _>>()?,
        )),
        TypeExpr::Constructor(constructor) => Ok(TypeExpr::Constructor(constructor.clone())),
        TypeExpr::Record(record) => Ok(TypeExpr::Record(ozzy_types::syntax::RecordExpr {
            fields: record
                .fields
                .iter()
                .map(|field| {
                    Ok(ozzy_types::syntax::RecordField {
                        name: field.name.clone(),
                        optional: field.optional,
                        ty: expand_local_type_expr(
                            definitions,
                            external_canonical_exprs,
                            &field.ty,
                            stack,
                        )?,
                    })
                })
                .collect::<Result<Vec<_>, PublicationError>>()?,
            open: record.open,
        })),
        TypeExpr::Collection(item) => Ok(TypeExpr::Collection(Box::new(expand_local_type_expr(
            definitions,
            external_canonical_exprs,
            item,
            stack,
        )?))),
        TypeExpr::Table(row) => Ok(TypeExpr::Table(Box::new(expand_local_type_expr(
            definitions,
            external_canonical_exprs,
            row,
            stack,
        )?))),
    }
}

fn next_numeric_version<'a>(
    entity: &'static str,
    name: &str,
    versions: impl Iterator<Item = &'a str>,
) -> Result<String, PublicationError> {
    let mut max_version = 0_u64;

    for version in versions {
        let parsed = version
            .parse::<u64>()
            .map_err(|_| PublicationError::NonNumericVersion {
                entity,
                name: name.to_string(),
                version: version.to_string(),
            })?;
        max_version = max_version.max(parsed);
    }

    Ok((max_version + 1).to_string())
}

async fn get_commit_by_sha_tx(
    tx: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    sha: &str,
) -> Result<Option<Commit>, sqlx::Error> {
    sqlx::query_as::<_, Commit>(
        "SELECT * FROM commits WHERE project_id = $1 AND git_commit_sha = $2",
    )
    .bind(project_id)
    .bind(sha)
    .fetch_optional(&mut **tx)
    .await
}

async fn insert_commit_tx(
    tx: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    git_provider: &str,
    git_repo: &str,
    git_commit_sha: &str,
    ozzy_toml_hash: &str,
    pushed_by: Uuid,
    message: Option<&str>,
) -> Result<Commit, sqlx::Error> {
    sqlx::query_as::<_, Commit>(
        r#"
        INSERT INTO commits (project_id, git_provider, git_repo, git_commit_sha, ozzy_toml_hash, pushed_by, message)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING *
        "#,
    )
    .bind(project_id)
    .bind(git_provider)
    .bind(git_repo)
    .bind(git_commit_sha)
    .bind(ozzy_toml_hash)
    .bind(pushed_by)
    .bind(message)
    .fetch_one(&mut **tx)
    .await
}

async fn upsert_ref_tx(
    tx: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    ref_name: &str,
    ref_type: &str,
    commit_id: Uuid,
) -> Result<Ref, sqlx::Error> {
    sqlx::query_as::<_, Ref>(
        r#"
        INSERT INTO refs (id, project_id, ref_name, ref_type, commit_id)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (project_id, ref_name) DO UPDATE SET
            commit_id = EXCLUDED.commit_id,
            ref_type = EXCLUDED.ref_type,
            updated_at = now()
        RETURNING *
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(project_id)
    .bind(ref_name)
    .bind(ref_type)
    .bind(commit_id)
    .fetch_one(&mut **tx)
    .await
}

async fn get_project_revision_by_commit_tx(
    tx: &mut Transaction<'_, Postgres>,
    source_commit_id: Uuid,
) -> Result<Option<StoredProjectRevision>, sqlx::Error> {
    sqlx::query_as::<_, StoredProjectRevision>(
        "SELECT * FROM v4_project_revisions WHERE source_commit_id = $1",
    )
    .bind(source_commit_id)
    .fetch_optional(&mut **tx)
    .await
}

async fn get_type_version_tx(
    tx: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    name: &str,
    version: &str,
) -> Result<Option<StoredTypeVersion>, sqlx::Error> {
    sqlx::query_as::<_, StoredTypeVersion>(
        r#"
        SELECT *
        FROM v4_type_versions
        WHERE project_id = $1 AND name = $2 AND version = $3
        "#,
    )
    .bind(project_id)
    .bind(name)
    .bind(version)
    .fetch_optional(&mut **tx)
    .await
}

async fn list_type_versions_by_name_tx(
    tx: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    name: &str,
) -> Result<Vec<StoredTypeVersion>, sqlx::Error> {
    sqlx::query_as::<_, StoredTypeVersion>(
        r#"
        SELECT *
        FROM v4_type_versions
        WHERE project_id = $1 AND name = $2
        ORDER BY created_at DESC
        "#,
    )
    .bind(project_id)
    .bind(name)
    .fetch_all(&mut **tx)
    .await
}

async fn get_canonical_type_by_key_tx(
    tx: &mut Transaction<'_, Postgres>,
    canonical_key: &str,
) -> Result<Option<StoredCanonicalType>, sqlx::Error> {
    sqlx::query_as::<_, StoredCanonicalType>(
        "SELECT * FROM v4_canonical_types WHERE canonical_key = $1",
    )
    .bind(canonical_key)
    .fetch_optional(&mut **tx)
    .await
}

async fn get_canonical_type_by_id_tx(
    tx: &mut Transaction<'_, Postgres>,
    canonical_type_id: Uuid,
) -> Result<Option<StoredCanonicalType>, sqlx::Error> {
    sqlx::query_as::<_, StoredCanonicalType>("SELECT * FROM v4_canonical_types WHERE id = $1")
        .bind(canonical_type_id)
        .fetch_optional(&mut **tx)
        .await
}

async fn resolve_or_insert_canonical_type(
    tx: &mut Transaction<'_, Postgres>,
    canonical: &CanonicalType,
) -> Result<StoredCanonicalType, sqlx::Error> {
    if let Some(existing) = get_canonical_type_by_key_tx(tx, canonical.id.as_str()).await? {
        return Ok(existing);
    }

    sqlx::query_as::<_, StoredCanonicalType>(
        r#"
        INSERT INTO v4_canonical_types (canonical_key, expr)
        VALUES ($1, $2)
        RETURNING *
        "#,
    )
    .bind(canonical.id.as_str())
    .bind(
        serde_json::to_value(&canonical.expr)
            .map_err(|err| sqlx::Error::Protocol(err.to_string()))?,
    )
    .fetch_one(&mut **tx)
    .await
}

async fn insert_type_version_tx(
    tx: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    published_by: Uuid,
    type_version: &TypeVersion,
    canonical_type_id: Uuid,
) -> Result<StoredTypeVersion, sqlx::Error> {
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
    .bind(
        serde_json::to_value(&type_version.expr)
            .map_err(|err| sqlx::Error::Protocol(err.to_string()))?,
    )
    .bind(published_by)
    .fetch_one(&mut **tx)
    .await
}

async fn list_environment_versions_by_name_tx(
    tx: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    name: &str,
) -> Result<Vec<StoredEnvironmentVersion>, sqlx::Error> {
    sqlx::query_as::<_, StoredEnvironmentVersion>(
        r#"
        SELECT *
        FROM v4_environment_versions
        WHERE project_id = $1 AND name = $2
        ORDER BY created_at DESC
        "#,
    )
    .bind(project_id)
    .bind(name)
    .fetch_all(&mut **tx)
    .await
}

async fn insert_environment_version_tx(
    tx: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    published_by: Uuid,
    name: &str,
    version: &str,
    definition: &Value,
) -> Result<StoredEnvironmentVersion, sqlx::Error> {
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
    .fetch_one(&mut **tx)
    .await
}

async fn list_transform_versions_by_name_tx(
    tx: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    name: &str,
) -> Result<Vec<StoredTransformVersion>, sqlx::Error> {
    sqlx::query_as::<_, StoredTransformVersion>(
        r#"
        SELECT *
        FROM v4_transform_versions
        WHERE project_id = $1 AND name = $2
        ORDER BY created_at DESC
        "#,
    )
    .bind(project_id)
    .bind(name)
    .fetch_all(&mut **tx)
    .await
}

async fn insert_transform_version_tx(
    tx: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    published_by: Uuid,
    name: &str,
    version: &str,
    environment_version_id: Uuid,
    transform: &TransformDef,
    params_schema: &Value,
) -> Result<StoredTransformVersion, sqlx::Error> {
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
    .bind(&transform.source)
    .bind(&transform.command)
    .bind(&transform.description)
    .bind(params_schema)
    .bind(transform.network)
    .bind(&transform.secrets)
    .bind(published_by)
    .fetch_one(&mut **tx)
    .await
}

async fn list_transform_ports_tx(
    tx: &mut Transaction<'_, Postgres>,
    transform_version_id: Uuid,
) -> Result<Vec<StoredTransformPort>, sqlx::Error> {
    sqlx::query_as::<_, StoredTransformPort>(
        r#"
        SELECT *
        FROM v4_transform_ports
        WHERE transform_version_id = $1
        ORDER BY port_kind, port_name
        "#,
    )
    .bind(transform_version_id)
    .fetch_all(&mut **tx)
    .await
}

async fn insert_transform_ports_tx(
    tx: &mut Transaction<'_, Postgres>,
    transform_version_id: Uuid,
    input_rows: &BTreeMap<String, StoredTypeVersion>,
    output_rows: &BTreeMap<String, StoredTypeVersion>,
    authored_inputs: &TypedPortSet,
    authored_outputs: &TypedPortSet,
) -> Result<(), sqlx::Error> {
    for (port_name, row) in input_rows {
        sqlx::query(
            r#"
            INSERT INTO v4_transform_ports (
                transform_version_id,
                port_kind,
                port_name,
                type_version_id,
                description
            )
            VALUES ($1, 'input', $2, $3, $4)
            "#,
        )
        .bind(transform_version_id)
        .bind(port_name)
        .bind(row.id)
        .bind(
            authored_inputs
                .ports
                .get(port_name)
                .and_then(|port| port.description.as_deref()),
        )
        .execute(&mut **tx)
        .await?;
    }

    for (port_name, row) in output_rows {
        sqlx::query(
            r#"
            INSERT INTO v4_transform_ports (
                transform_version_id,
                port_kind,
                port_name,
                type_version_id,
                description
            )
            VALUES ($1, 'output', $2, $3, $4)
            "#,
        )
        .bind(transform_version_id)
        .bind(port_name)
        .bind(row.id)
        .bind(
            authored_outputs
                .ports
                .get(port_name)
                .and_then(|port| port.description.as_deref()),
        )
        .execute(&mut **tx)
        .await?;
    }

    Ok(())
}

async fn insert_registry_revision_tx(
    tx: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    created_by: Uuid,
) -> Result<crate::db::v4::StoredRegistryRevision, sqlx::Error> {
    sqlx::query_as::<_, crate::db::v4::StoredRegistryRevision>(
        r#"
        INSERT INTO v4_registry_revisions (project_id, created_by)
        VALUES ($1, $2)
        RETURNING *
        "#,
    )
    .bind(project_id)
    .bind(created_by)
    .fetch_one(&mut **tx)
    .await
}

async fn attach_type_version_tx(
    tx: &mut Transaction<'_, Postgres>,
    registry_revision_id: Uuid,
    type_version_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO v4_registry_revision_type_versions (registry_revision_id, type_version_id)
        VALUES ($1, $2)
        ON CONFLICT DO NOTHING
        "#,
    )
    .bind(registry_revision_id)
    .bind(type_version_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn attach_environment_version_tx(
    tx: &mut Transaction<'_, Postgres>,
    registry_revision_id: Uuid,
    environment_version_id: Uuid,
) -> Result<(), sqlx::Error> {
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
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn attach_transform_version_tx(
    tx: &mut Transaction<'_, Postgres>,
    registry_revision_id: Uuid,
    transform_version_id: Uuid,
) -> Result<(), sqlx::Error> {
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
    .execute(&mut **tx)
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn insert_project_revision_tx(
    tx: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    source_commit_id: Uuid,
    registry_revision_id: Uuid,
    ozzy_toml_hash: &str,
    ozzy_toml_raw: &str,
    environments: &Value,
    transforms: &Value,
    endpoints: &Value,
    project_meta: &Value,
    created_by: Uuid,
) -> Result<StoredProjectRevision, sqlx::Error> {
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
    .fetch_one(&mut **tx)
    .await
}

#[cfg(test)]
mod tests {
    use std::env;

    use sqlx::postgres::PgPoolOptions;

    use super::*;
    use crate::db::Database;

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
    async fn publication_reuses_equivalent_type_environment_and_transform_versions() {
        let Some(db) = get_test_db().await else {
            return;
        };

        let user = db
            .upsert_user_from_github(
                rand::random::<i64>() & i64::MAX,
                &unique_name("pubuser"),
                None,
                None,
            )
            .await
            .expect("create user");
        let project = db
            .create_project(
                user.id,
                &unique_name("pubproj"),
                Some("publication test"),
                "private",
            )
            .await
            .expect("create project");

        let toml = format!(
            r#"
[project]
name = "{slug}"
owner = "{owner}"

[types]
CsvRow = '{{ value: float64 }}'
CsvData = 'csv(delimiter=",", header=true) & table<CsvRow>'

[environments.python]
image = "python:3.12-slim"

[transforms.clean]
command = "python -c 'print(1)'"
environment = "python"

[transforms.clean.inputs.raw]
type = "CsvData"

[transforms.clean.outputs.result]
type = "CsvData"

[endpoints.run]
[endpoints.run.nodes.clean]
transform = "clean"
"#,
            slug = unique_name("slug"),
            owner = user.username,
        );

        let ozzy_toml = OzzyToml::parse(&toml).expect("parse ozzy.toml");
        assert!(ozzy_toml.validate().is_empty(), "validation should pass");
        let published_environments = BTreeMap::from([(
            "python".to_string(),
            PublishedEnvironmentDef::Prebuilt {
                image: "python:3.12-slim".to_string(),
            },
        )]);

        let input_1 = PublishCommitInput {
            project_id: project.id,
            pushed_by: user.id,
            git_provider: "github",
            git_repo: "rileyleff/ozzydb",
            git_commit_sha: &format!("{:040x}", rand::random::<u64>()),
            ozzy_toml_hash: "hash_1",
            message: Some("first"),
            ref_name: Some("main"),
            ozzy_toml_raw: &toml,
            ozzy_toml: &ozzy_toml,
            published_environments: &published_environments,
        };
        let first = publish_v4_commit_atomically(&db, input_1)
            .await
            .expect("first publication should succeed");
        let PublishOutcome::Created {
            bundle: first_bundle,
            ..
        } = first
        else {
            panic!("first publication should create");
        };

        let input_2 = PublishCommitInput {
            project_id: project.id,
            pushed_by: user.id,
            git_provider: "github",
            git_repo: "rileyleff/ozzydb",
            git_commit_sha: &format!("{:040x}", rand::random::<u64>()),
            ozzy_toml_hash: "hash_2",
            message: Some("second"),
            ref_name: Some("main"),
            ozzy_toml_raw: &toml,
            ozzy_toml: &ozzy_toml,
            published_environments: &published_environments,
        };
        let second = publish_v4_commit_atomically(&db, input_2)
            .await
            .expect("second publication should succeed");
        let PublishOutcome::Created {
            bundle: second_bundle,
            ..
        } = second
        else {
            panic!("second publication should create");
        };

        assert_eq!(
            first_bundle.types["CsvData"].id,
            second_bundle.types["CsvData"].id
        );
        assert_eq!(
            first_bundle.environments["python"].id,
            second_bundle.environments["python"].id
        );
        assert_eq!(
            first_bundle.transforms["clean"].id,
            second_bundle.transforms["clean"].id
        );
    }
}
