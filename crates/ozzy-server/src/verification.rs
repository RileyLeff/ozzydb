//! Artifact-backed verification helpers for the v4 runtime path.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::registry::RegistrySnapshot;
use crate::{
    AppState,
    db::v4::{StoredArtifact, StoredConformanceRecord},
};
use async_recursion::async_recursion;
use chrono::{DateTime, NaiveDate, NaiveDateTime};
use ozzy_types::conformance::{VerificationAttempt, VerificationFailure};
use ozzy_types::registry::TypeRegistry;
use ozzy_types::syntax::{
    BuiltinConstructor, BuiltinType, ConstructorExpr, Literal, TypeDefinitions, TypeExpr,
    TypeRefExpr,
};
use ozzy_types::verify::{
    BuiltinVerifierRegistry, CsvWitness, TableColumnWitness, TableWitness, VerificationInput,
    VerificationReport,
};

#[derive(Debug, thiserror::Error)]
pub enum ArtifactVerificationError {
    #[error(transparent)]
    Registry(#[from] crate::registry::RegistrySnapshotError),
    #[error(transparent)]
    Verify(#[from] ozzy_types::verify::VerificationError),
    #[error(transparent)]
    Csv(#[from] csv::Error),
    #[error(transparent)]
    SerdeJson(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Db(#[from] crate::db::v4::V4QueryError),
    #[error(transparent)]
    Storage(#[from] anyhow::Error),
    #[error("blob artifact '{artifact_id}' is missing a content hash")]
    MissingContentHash { artifact_id: uuid::Uuid },
    #[error("type '{type_name}' does not declare a verifiable blob encoding")]
    MissingBlobEncoding { type_name: String },
    #[error(
        "artifact '{artifact_id}' with kind '{artifact_kind}' cannot satisfy type '{type_name}'"
    )]
    ArtifactKindMismatch {
        artifact_id: uuid::Uuid,
        artifact_kind: String,
        type_name: String,
    },
    #[error(
        "record type '{type_name}' requires field '{field}' but manifest artifact '{artifact_id}' does not provide it"
    )]
    MissingManifestField {
        artifact_id: uuid::Uuid,
        type_name: String,
        field: String,
    },
    #[error(
        "open record type '{type_name}' is not yet verifiable against manifest artifact '{artifact_id}'"
    )]
    OpenManifestRecordUnsupported {
        artifact_id: uuid::Uuid,
        type_name: String,
    },
    #[error("expected collection type for manifest artifact '{artifact_id}', got '{type_name}'")]
    ExpectedCollectionType {
        artifact_id: uuid::Uuid,
        type_name: String,
    },
    #[error("expected record type for manifest artifact '{artifact_id}', got '{type_name}'")]
    ExpectedRecordType {
        artifact_id: uuid::Uuid,
        type_name: String,
    },
    #[error("CSV constructor on '{type_name}' is missing or malformed")]
    InvalidCsvType { type_name: String },
}

impl ArtifactVerificationError {
    pub fn as_failure(&self) -> VerificationAttempt {
        VerificationAttempt::Failed(VerificationFailure {
            verifier: "builtin.v1".to_string(),
            error: self.to_string(),
        })
    }
}

pub async fn verify_stored_artifact(
    state: &AppState,
    snapshot: &RegistrySnapshot,
    artifact: &StoredArtifact,
    type_ref: &TypeRefExpr,
) -> Result<VerificationReport, ArtifactVerificationError> {
    let expr = snapshot.expanded_type_expr(type_ref)?;
    let scratch = tempfile::tempdir()?;
    let input =
        build_verification_input_for_stored_artifact(state, artifact, &expr, scratch.path())
            .await?;
    verify_input(snapshot.type_registry(), &expr, &input)
}

pub fn verify_output_bytes(
    snapshot: &RegistrySnapshot,
    type_ref: &TypeRefExpr,
    content_type: &str,
    output_bytes: &[u8],
) -> Result<VerificationReport, ArtifactVerificationError> {
    let expr = snapshot.expanded_type_expr(type_ref)?;
    let scratch = tempfile::tempdir()?;
    let input = build_verification_input_from_blob_bytes(
        output_bytes,
        Some(content_type),
        &expr,
        scratch.path(),
    )?;
    verify_input(snapshot.type_registry(), &expr, &input)
}

pub async fn ensure_conformance_verified(
    state: &AppState,
    snapshot: &RegistrySnapshot,
    artifact: &StoredArtifact,
    conformance: &StoredConformanceRecord,
    type_ref: &TypeRefExpr,
) -> Result<StoredConformanceRecord, ArtifactVerificationError> {
    match conformance.status.as_str() {
        "verified" => Ok(conformance.clone()),
        "rejected" => Ok(conformance.clone()),
        "declared" => {
            let report = match verify_stored_artifact(state, snapshot, artifact, type_ref).await {
                Ok(report) => report,
                Err(err) => {
                    state
                        .db
                        .record_v4_verification_failure(conformance.id, &err.as_failure())
                        .await?;
                    return Err(err);
                }
            };

            state
                .db
                .record_v4_verification_report(conformance.id, &report)
                .await?;

            let updated = state
                .db
                .get_v4_conformance_record(artifact.id, conformance.type_version_id)
                .await?
                .ok_or_else(|| ArtifactVerificationError::Db(crate::db::v4::V4QueryError::InvalidInput(
                    format!(
                        "conformance record for artifact {} and type version {} disappeared after verification",
                        artifact.id, conformance.type_version_id
                    ),
                )))?;

            Ok(updated)
        }
        other => Err(ArtifactVerificationError::Db(
            crate::db::v4::V4QueryError::InvalidInput(format!(
                "unknown conformance status '{}'",
                other
            )),
        )),
    }
}

fn verify_input(
    registry: &TypeRegistry,
    expr: &TypeExpr,
    input: &VerificationInput,
) -> Result<VerificationReport, ArtifactVerificationError> {
    let verifier = BuiltinVerifierRegistry;
    Ok(verifier.verify(&TypeDefinitions::default(), registry, expr, input)?)
}

#[async_recursion]
async fn build_verification_input_for_stored_artifact(
    state: &AppState,
    artifact: &StoredArtifact,
    expr: &TypeExpr,
    scratch_root: &Path,
) -> Result<VerificationInput, ArtifactVerificationError> {
    match artifact.artifact_kind.as_str() {
        "blob" => {
            let content_hash = artifact.content_hash.as_ref().ok_or(
                ArtifactVerificationError::MissingContentHash {
                    artifact_id: artifact.id,
                },
            )?;
            let ext = infer_blob_extension(expr).ok_or_else(|| {
                ArtifactVerificationError::MissingBlobEncoding {
                    type_name: describe_type_expr(expr),
                }
            })?;
            let bytes = state.storage.get(content_hash, ext).await?;
            build_verification_input_from_blob_bytes(&bytes, None, expr, scratch_root)
        }
        "manifest" => {
            let manifest = state.db.decode_v4_artifact_manifest(artifact)?;
            if let Some(record_expr) = find_record_expr(expr) {
                if record_expr.open {
                    return Err(ArtifactVerificationError::OpenManifestRecordUnsupported {
                        artifact_id: artifact.id,
                        type_name: describe_type_expr(expr),
                    });
                }

                let ozzy_core::artifacts::ArtifactManifest::Bundle { entries } = manifest else {
                    return Err(ArtifactVerificationError::ExpectedRecordType {
                        artifact_id: artifact.id,
                        type_name: describe_type_expr(expr),
                    });
                };

                let entry_map = entries
                    .into_iter()
                    .map(|(name, entry)| (name, entry.artifact_id))
                    .collect::<BTreeMap<_, _>>();
                let mut record = BTreeMap::new();
                for field in &record_expr.fields {
                    let Some(child_id) = entry_map.get(&field.name) else {
                        if field.optional {
                            continue;
                        }
                        return Err(ArtifactVerificationError::MissingManifestField {
                            artifact_id: artifact.id,
                            type_name: describe_type_expr(expr),
                            field: field.name.clone(),
                        });
                    };
                    let child = state.db.get_v4_artifact(*child_id).await?.ok_or_else(|| {
                        ArtifactVerificationError::Db(crate::db::v4::V4QueryError::InvalidInput(
                            format!(
                                "manifest artifact {} references missing artifact {} during verification",
                                artifact.id, child_id
                            ),
                        ))
                    })?;
                    let child_input = build_verification_input_for_stored_artifact(
                        state,
                        &child,
                        &field.ty,
                        scratch_root,
                    )
                    .await?;
                    record.insert(field.name.clone(), child_input);
                }
                Ok(VerificationInput::Record(record))
            } else if let Some(item_expr) = find_collection_item_expr(expr) {
                let ozzy_core::artifacts::ArtifactManifest::Collection { items } = manifest else {
                    return Err(ArtifactVerificationError::ExpectedCollectionType {
                        artifact_id: artifact.id,
                        type_name: describe_type_expr(expr),
                    });
                };
                let mut values = Vec::with_capacity(items.len());
                for item in items {
                    let child = state.db.get_v4_artifact(item.artifact_id).await?.ok_or_else(|| {
                        ArtifactVerificationError::Db(crate::db::v4::V4QueryError::InvalidInput(
                            format!(
                                "manifest artifact {} references missing artifact {} during verification",
                                artifact.id, item.artifact_id
                            ),
                        ))
                    })?;
                    values.push(
                        build_verification_input_for_stored_artifact(
                            state,
                            &child,
                            item_expr,
                            scratch_root,
                        )
                        .await?,
                    );
                }
                Ok(VerificationInput::Collection(values))
            } else {
                Err(ArtifactVerificationError::ArtifactKindMismatch {
                    artifact_id: artifact.id,
                    artifact_kind: artifact.artifact_kind.clone(),
                    type_name: describe_type_expr(expr),
                })
            }
        }
        kind => Err(ArtifactVerificationError::ArtifactKindMismatch {
            artifact_id: artifact.id,
            artifact_kind: kind.to_string(),
            type_name: describe_type_expr(expr),
        }),
    }
}

fn build_verification_input_from_blob_bytes(
    bytes: &[u8],
    content_type: Option<&str>,
    expr: &TypeExpr,
    scratch_dir: &Path,
) -> Result<VerificationInput, ArtifactVerificationError> {
    let mut derived = vec![VerificationInput::Bytes(bytes.to_vec())];

    if let Some(csv_constructor) = find_csv_constructor(expr) {
        extend_derived_input(
            &mut derived,
            build_csv_verification_input(bytes, csv_constructor)?,
        );
    }

    if contains_builtin(expr, BuiltinType::Parquet)
        || content_type == Some("application/vnd.apache.parquet")
    {
        let path = write_temp_file(scratch_dir, "parquet", bytes)?;
        derived.push(VerificationInput::ParquetFile(path));
    }

    if contains_builtin(expr, BuiltinType::Json) || content_type == Some("application/json") {
        let json = serde_json::from_slice::<serde_json::Value>(bytes)?;
        extend_derived_input(&mut derived, json_to_verification_input(&json)?);
    }

    if contains_builtin(expr, BuiltinType::Utf8) || contains_builtin(expr, BuiltinType::String) {
        let text = std::str::from_utf8(bytes).map_err(|err| {
            ArtifactVerificationError::Storage(anyhow::anyhow!(
                "failed to decode UTF-8 text during verification: {}",
                err
            ))
        })?;
        derived.push(VerificationInput::Scalar(Literal::String(text.to_string())));
    }

    if derived.len() == 1 && !contains_builtin(expr, BuiltinType::Bytes) {
        return Err(ArtifactVerificationError::MissingBlobEncoding {
            type_name: describe_type_expr(expr),
        });
    }

    Ok(if derived.len() == 1 {
        derived.remove(0)
    } else {
        VerificationInput::Derived(derived)
    })
}

fn build_csv_verification_input(
    bytes: &[u8],
    constructor: &ConstructorExpr,
) -> Result<VerificationInput, ArtifactVerificationError> {
    let delimiter = match constructor.args.get("delimiter") {
        Some(Literal::String(s)) if s.len() == 1 => s.as_bytes()[0],
        _ => {
            return Err(ArtifactVerificationError::InvalidCsvType {
                type_name: "csv".to_string(),
            });
        }
    };
    let header = match constructor.args.get("header") {
        Some(Literal::Bool(value)) => *value,
        _ => {
            return Err(ArtifactVerificationError::InvalidCsvType {
                type_name: "csv".to_string(),
            });
        }
    };

    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(header)
        .from_reader(bytes);

    let mut headers: Vec<String> = Vec::new();
    let mut inferred_types: Vec<InferredColumnType> = Vec::new();
    let mut nullable: Vec<bool> = Vec::new();
    let mut row_count: u64 = 0;

    if header {
        headers = reader
            .headers()?
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        inferred_types.resize(headers.len(), InferredColumnType::Unknown);
        nullable.resize(headers.len(), false);
    }

    for record in reader.records() {
        let record = record?;
        row_count += 1;
        if !header && headers.is_empty() {
            headers = (0..record.len())
                .map(|idx| format!("column_{idx}"))
                .collect::<Vec<_>>();
            inferred_types.resize(record.len(), InferredColumnType::Unknown);
            nullable.resize(record.len(), false);
        }
        if record.len() > inferred_types.len() {
            let start = inferred_types.len();
            inferred_types.resize(record.len(), InferredColumnType::Unknown);
            nullable.resize(record.len(), false);
            headers.extend((start..record.len()).map(|idx| format!("column_{idx}")));
        }

        for (idx, value) in record.iter().enumerate() {
            if value.is_empty() {
                nullable[idx] = true;
                continue;
            }
            let inferred = infer_scalar_type(value);
            inferred_types[idx] = inferred_types[idx].merge(inferred);
        }
    }

    let csv_witness = CsvWitness {
        delimiter: char::from(delimiter).to_string(),
        header,
        columns: headers.clone(),
        row_count: Some(row_count),
    };
    let table = TableWitness {
        columns: headers
            .into_iter()
            .enumerate()
            .map(|(idx, name)| TableColumnWitness {
                name,
                data_type: inferred_types[idx].to_table_dtype().to_string(),
                nullable: nullable[idx],
            })
            .collect(),
        row_count: Some(row_count),
    };

    Ok(VerificationInput::Derived(vec![
        VerificationInput::Csv(csv_witness),
        VerificationInput::Table(table),
    ]))
}

fn json_to_verification_input(
    value: &serde_json::Value,
) -> Result<VerificationInput, ArtifactVerificationError> {
    let json_marker = VerificationInput::Json(value.clone());
    match value {
        serde_json::Value::Null => Ok(json_marker),
        serde_json::Value::Bool(v) => Ok(VerificationInput::Derived(vec![
            json_marker,
            VerificationInput::Scalar(Literal::Bool(*v)),
        ])),
        serde_json::Value::Number(v) => {
            if let Some(i) = v.as_i64() {
                Ok(VerificationInput::Derived(vec![
                    json_marker,
                    VerificationInput::Scalar(Literal::Integer(i)),
                ]))
            } else if let Some(f) = v.as_f64() {
                Ok(VerificationInput::Derived(vec![
                    json_marker,
                    VerificationInput::Scalar(Literal::Float(f.into())),
                ]))
            } else {
                Err(ArtifactVerificationError::Storage(anyhow::anyhow!(
                    "unsupported JSON number representation during verification"
                )))
            }
        }
        serde_json::Value::String(v) => Ok(VerificationInput::Derived(vec![
            json_marker,
            VerificationInput::Scalar(Literal::String(v.clone())),
        ])),
        serde_json::Value::Array(values) => Ok(VerificationInput::Derived(vec![
            json_marker,
            VerificationInput::Collection(
                values
                    .iter()
                    .map(json_to_verification_input)
                    .collect::<Result<Vec<_>, _>>()?,
            ),
        ])),
        serde_json::Value::Object(values) => {
            let mut record = BTreeMap::new();
            for (key, value) in values {
                record.insert(key.clone(), json_to_verification_input(value)?);
            }
            Ok(VerificationInput::Derived(vec![
                json_marker,
                VerificationInput::Record(record),
            ]))
        }
    }
}

fn extend_derived_input(target: &mut Vec<VerificationInput>, input: VerificationInput) {
    match input {
        VerificationInput::Derived(inputs) => target.extend(inputs),
        other => target.push(other),
    }
}

fn write_temp_file(
    dir: &Path,
    extension: &str,
    bytes: &[u8],
) -> Result<PathBuf, ArtifactVerificationError> {
    let path = dir.join(format!(
        "verification-{}.{}",
        uuid::Uuid::new_v4(),
        extension
    ));
    std::fs::write(&path, bytes)?;
    Ok(path)
}

fn infer_blob_extension(expr: &TypeExpr) -> Option<&'static str> {
    if find_csv_constructor(expr).is_some() {
        return Some("csv");
    }
    if contains_builtin(expr, BuiltinType::Parquet) {
        return Some("parquet");
    }
    if contains_builtin(expr, BuiltinType::Json) {
        return Some("json");
    }
    if contains_builtin(expr, BuiltinType::Utf8) || contains_builtin(expr, BuiltinType::String) {
        return Some("txt");
    }
    if contains_builtin(expr, BuiltinType::Bytes) {
        return Some("bin");
    }
    None
}

fn find_csv_constructor(expr: &TypeExpr) -> Option<&ConstructorExpr> {
    match expr {
        TypeExpr::Constructor(constructor) if constructor.name == BuiltinConstructor::Csv => {
            Some(constructor)
        }
        TypeExpr::Intersection(parts) => parts.iter().find_map(find_csv_constructor),
        _ => None,
    }
}

fn find_record_expr(expr: &TypeExpr) -> Option<&ozzy_types::syntax::RecordExpr> {
    match expr {
        TypeExpr::Record(record) => Some(record),
        TypeExpr::Intersection(parts) => parts.iter().find_map(find_record_expr),
        _ => None,
    }
}

fn find_collection_item_expr(expr: &TypeExpr) -> Option<&TypeExpr> {
    match expr {
        TypeExpr::Collection(item) => Some(item.as_ref()),
        TypeExpr::Intersection(parts) => parts.iter().find_map(find_collection_item_expr),
        _ => None,
    }
}

fn contains_builtin(expr: &TypeExpr, builtin: BuiltinType) -> bool {
    match expr {
        TypeExpr::Builtin(value) => *value == builtin,
        TypeExpr::Intersection(parts) => parts.iter().any(|part| contains_builtin(part, builtin)),
        _ => false,
    }
}

fn describe_type_expr(expr: &TypeExpr) -> String {
    serde_json::to_string(expr).unwrap_or_else(|_| "<type-expr>".to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InferredColumnType {
    Unknown,
    Bool,
    Int64,
    Float64,
    Date,
    DateTime,
    Utf8,
}

impl InferredColumnType {
    fn merge(self, other: Self) -> Self {
        use InferredColumnType as T;
        match (self, other) {
            (T::Unknown, rhs) => rhs,
            (lhs, T::Unknown) => lhs,
            (T::Int64, T::Float64) | (T::Float64, T::Int64) => T::Float64,
            (T::Date, T::DateTime) | (T::DateTime, T::Date) => T::DateTime,
            (lhs, rhs) if lhs == rhs => lhs,
            _ => T::Utf8,
        }
    }

    fn to_table_dtype(self) -> &'static str {
        match self {
            Self::Unknown | Self::Utf8 => "utf8",
            Self::Bool => "bool",
            Self::Int64 => "int64",
            Self::Float64 => "float64",
            Self::Date => "date32",
            Self::DateTime => "timestamp[us]",
        }
    }
}

fn infer_scalar_type(value: &str) -> InferredColumnType {
    let trimmed = value.trim();
    if trimmed.eq_ignore_ascii_case("true") || trimmed.eq_ignore_ascii_case("false") {
        return InferredColumnType::Bool;
    }
    if trimmed.parse::<i64>().is_ok() {
        return InferredColumnType::Int64;
    }
    if trimmed.parse::<f64>().is_ok() {
        return InferredColumnType::Float64;
    }
    if NaiveDate::parse_from_str(trimmed, "%Y-%m-%d").is_ok() {
        return InferredColumnType::Date;
    }
    if DateTime::parse_from_rfc3339(trimmed).is_ok()
        || NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S").is_ok()
        || NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%dT%H:%M:%S").is_ok()
    {
        return InferredColumnType::DateTime;
    }
    InferredColumnType::Utf8
}

#[cfg(test)]
mod tests {
    use super::*;
    use ozzy_types::syntax::TypeExpr;

    #[test]
    fn blob_bytes_support_bytes_types() {
        let expr = TypeExpr::ref_("bytes");
        let input =
            build_verification_input_from_blob_bytes(b"\x00\x01", None, &expr, Path::new("/tmp"))
                .expect("bytes input");

        match input {
            VerificationInput::Bytes(bytes) => assert_eq!(bytes, b"\x00\x01"),
            other => panic!("expected raw bytes input, got {other:?}"),
        }
    }

    #[test]
    fn blob_bytes_derive_json_and_structure() {
        let expr = TypeExpr::intersection(vec![
            TypeExpr::ref_("json"),
            TypeExpr::Record(ozzy_types::syntax::RecordExpr {
                fields: vec![ozzy_types::syntax::RecordField {
                    name: "site_id".to_string(),
                    ty: TypeExpr::ref_("string"),
                    optional: false,
                }],
                open: false,
            }),
        ])
        .expect("intersection");

        let input = build_verification_input_from_blob_bytes(
            br#"{"site_id":"east30"}"#,
            Some("application/json"),
            &expr,
            Path::new("/tmp"),
        )
        .expect("json input");

        match input {
            VerificationInput::Derived(values) => {
                assert!(
                    values
                        .iter()
                        .any(|value| matches!(value, VerificationInput::Json(_)))
                );
                assert!(
                    values
                        .iter()
                        .any(|value| matches!(value, VerificationInput::Record(_)))
                );
            }
            other => panic!("expected derived verification input, got {other:?}"),
        }
    }
}
