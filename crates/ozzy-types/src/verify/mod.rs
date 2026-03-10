//! Verification planning and builtin verification execution.

mod witness;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

use crate::canonical::{CanonicalizationError, canonicalize};
use crate::registry::{RegistryError, TypeRegistry};
use crate::syntax::{
    BuiltinConstructor, BuiltinType, ConstructorExpr, Literal, TypeDefinitions, TypeExpr,
    TypeRefExpr,
};

pub use witness::{CsvWitness, RecordWitness, TableColumnWitness, TableWitness, WitnessError};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerificationVerdict {
    Verified,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VerificationReport {
    pub verifier: String,
    pub verdict: VerificationVerdict,
    #[serde(default)]
    pub diagnostics: Vec<String>,
    #[serde(default)]
    pub evidence: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum VerificationInput {
    Scalar(Literal),
    Csv(CsvWitness),
    Table(TableWitness),
    TableColumn(TableColumnWitness),
    Record(BTreeMap<String, VerificationInput>),
    Collection(Vec<VerificationInput>),
    ParquetFile(PathBuf),
    Derived(Vec<VerificationInput>),
}

impl VerificationInput {
    fn kind_name(&self) -> &'static str {
        match self {
            Self::Scalar(_) => "scalar",
            Self::Csv(_) => "csv_witness",
            Self::Table(_) => "table_witness",
            Self::TableColumn(_) => "table_column_witness",
            Self::Record(_) => "record",
            Self::Collection(_) => "collection",
            Self::ParquetFile(_) => "parquet_file",
            Self::Derived(_) => "derived",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerificationPlan {
    All(Vec<VerificationPlan>),
    Builtin(BuiltinType),
    Constructor(ConstructorExpr),
    Record {
        open: bool,
        fields: Vec<RecordFieldPlan>,
    },
    Collection(Box<VerificationPlan>),
    Table(Box<VerificationPlan>),
    Never,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordFieldPlan {
    pub name: String,
    pub optional: bool,
    pub plan: Box<VerificationPlan>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct BuiltinVerifierRegistry;

#[derive(Debug, Error)]
pub enum VerificationError {
    #[error(transparent)]
    Canonicalization(#[from] CanonicalizationError),
    #[error(transparent)]
    Registry(#[from] RegistryError),
    #[error(transparent)]
    Witness(#[from] WitnessError),
    #[error("verification input kind '{actual}' cannot satisfy expected '{expected}'")]
    InputKindMismatch {
        expected: &'static str,
        actual: &'static str,
    },
    #[error("cannot verify unresolved external type reference '{name}'")]
    UnsupportedExternalRef { name: String },
    #[error("constraint '{constraint}' is not supported for {context}")]
    UnsupportedConstraint {
        constraint: String,
        context: &'static str,
    },
    #[error("constructor '{constructor}' is missing required argument '{arg}' during verification")]
    MissingConstructorArg {
        constructor: BuiltinConstructor,
        arg: &'static str,
    },
    #[error(
        "constructor '{constructor}' has invalid argument '{arg}' during verification; expected {expected}"
    )]
    InvalidConstructorArg {
        constructor: BuiltinConstructor,
        arg: &'static str,
        expected: &'static str,
    },
    #[error("published type reference cycle detected during verification planning: {cycle:?}")]
    RecursivePublishedTypeReference { cycle: Vec<String> },
    #[error("failed to serialize verification evidence")]
    EvidenceEncodingFailed(#[source] serde_json::Error),
}

impl BuiltinVerifierRegistry {
    pub fn compile(
        &self,
        defs: &TypeDefinitions,
        registry: &TypeRegistry,
        expr: &TypeExpr,
    ) -> Result<VerificationPlan, VerificationError> {
        let canonical = canonicalize(defs, expr)?;
        compile_plan(registry, &canonical, &mut Vec::new())
    }

    pub fn verify(
        &self,
        defs: &TypeDefinitions,
        registry: &TypeRegistry,
        expr: &TypeExpr,
        input: &VerificationInput,
    ) -> Result<VerificationReport, VerificationError> {
        let plan = self.compile(defs, registry, expr)?;
        let outcome = execute_plan(&plan, input)?;

        Ok(VerificationReport {
            verifier: "builtin.v1".to_string(),
            verdict: if outcome.verified {
                VerificationVerdict::Verified
            } else {
                VerificationVerdict::Rejected
            },
            diagnostics: outcome.diagnostics,
            evidence: Some(outcome.evidence),
        })
    }
}

fn compile_plan(
    registry: &TypeRegistry,
    expr: &TypeExpr,
    stack: &mut Vec<String>,
) -> Result<VerificationPlan, VerificationError> {
    match expr {
        TypeExpr::Intersection(parts) => Ok(VerificationPlan::All(
            parts
                .iter()
                .map(|part| compile_plan(registry, part, stack))
                .collect::<Result<Vec<_>, _>>()?,
        )),
        TypeExpr::Builtin(builtin) => Ok(VerificationPlan::Builtin(*builtin)),
        TypeExpr::Constructor(constructor) => {
            Ok(VerificationPlan::Constructor(constructor.clone()))
        }
        TypeExpr::Record(record) => Ok(VerificationPlan::Record {
            open: record.open,
            fields: record
                .fields
                .iter()
                .map(|field| {
                    Ok(RecordFieldPlan {
                        name: field.name.clone(),
                        optional: field.optional,
                        plan: Box::new(compile_plan(registry, &field.ty, stack)?),
                    })
                })
                .collect::<Result<Vec<_>, VerificationError>>()?,
        }),
        TypeExpr::Collection(item) => Ok(VerificationPlan::Collection(Box::new(compile_plan(
            registry, item, stack,
        )?))),
        TypeExpr::Table(row) => Ok(VerificationPlan::Table(Box::new(compile_plan(
            registry, row, stack,
        )?))),
        TypeExpr::Ref(type_ref) => compile_published_ref_plan(registry, type_ref, stack),
        TypeExpr::Never => Ok(VerificationPlan::Never),
    }
}

fn compile_published_ref_plan(
    registry: &TypeRegistry,
    type_ref: &TypeRefExpr,
    stack: &mut Vec<String>,
) -> Result<VerificationPlan, VerificationError> {
    let version =
        type_ref
            .version
            .as_ref()
            .ok_or_else(|| VerificationError::UnsupportedExternalRef {
                name: type_ref.name.clone(),
            })?;
    let key = format!("{}@{}", type_ref.name, version);

    if stack.contains(&key) {
        let mut cycle = stack.clone();
        cycle.push(key);
        return Err(VerificationError::RecursivePublishedTypeReference { cycle });
    }

    let type_version = registry.resolve_ref(type_ref)?;
    stack.push(format!("{}@{}", type_version.name, type_version.version));
    let result = compile_plan(registry, &type_version.expr, stack);
    stack.pop();
    result
}

#[derive(Debug)]
struct ExecutionOutcome {
    verified: bool,
    diagnostics: Vec<String>,
    evidence: Value,
}

impl ExecutionOutcome {
    fn verified(evidence: Value) -> Self {
        Self {
            verified: true,
            diagnostics: Vec::new(),
            evidence,
        }
    }

    fn rejected(message: impl Into<String>, evidence: Value) -> Self {
        Self {
            verified: false,
            diagnostics: vec![message.into()],
            evidence,
        }
    }

    fn merge_all(outcomes: Vec<ExecutionOutcome>) -> Self {
        let verified = outcomes.iter().all(|outcome| outcome.verified);
        let diagnostics = outcomes
            .iter()
            .flat_map(|outcome| outcome.diagnostics.clone())
            .collect::<Vec<_>>();
        let evidence = Value::Array(
            outcomes
                .into_iter()
                .map(|outcome| outcome.evidence)
                .collect(),
        );

        Self {
            verified,
            diagnostics,
            evidence,
        }
    }
}

fn execute_plan(
    plan: &VerificationPlan,
    input: &VerificationInput,
) -> Result<ExecutionOutcome, VerificationError> {
    if let VerificationInput::Derived(inputs) = input {
        return match plan {
            VerificationPlan::All(plans) => {
                let mut outcomes = Vec::with_capacity(plans.len());
                for plan in plans {
                    outcomes.push(execute_plan(plan, input)?);
                }

                Ok(ExecutionOutcome::merge_all(outcomes))
            }
            _ => execute_against_derived_inputs(plan, inputs),
        };
    }

    match plan {
        VerificationPlan::All(plans) => {
            let mut outcomes = Vec::with_capacity(plans.len());
            for plan in plans {
                outcomes.push(execute_plan(plan, input)?);
            }

            Ok(ExecutionOutcome::merge_all(outcomes))
        }
        VerificationPlan::Builtin(builtin) => verify_builtin(*builtin, input),
        VerificationPlan::Constructor(constructor) => verify_constructor(constructor, input),
        VerificationPlan::Record { open, fields } => verify_record(*open, fields, input),
        VerificationPlan::Collection(item_plan) => verify_collection(item_plan, input),
        VerificationPlan::Table(row_plan) => verify_table(row_plan, input),
        VerificationPlan::Never => Ok(ExecutionOutcome::rejected(
            "type canonicalized to never; no artifact can conform",
            json!({ "kind": "never" }),
        )),
    }
}

fn execute_against_derived_inputs(
    plan: &VerificationPlan,
    inputs: &[VerificationInput],
) -> Result<ExecutionOutcome, VerificationError> {
    let mut first_mismatch: Option<VerificationError> = None;

    for candidate in inputs {
        match execute_plan(plan, candidate) {
            Ok(outcome) => return Ok(outcome),
            Err(err @ VerificationError::InputKindMismatch { .. }) => {
                if first_mismatch.is_none() {
                    first_mismatch = Some(err);
                }
            }
            Err(other) => return Err(other),
        }
    }

    Err(
        first_mismatch.unwrap_or(VerificationError::InputKindMismatch {
            expected: "a compatible derived verification input",
            actual: "derived",
        }),
    )
}

fn verify_builtin(
    builtin: BuiltinType,
    input: &VerificationInput,
) -> Result<ExecutionOutcome, VerificationError> {
    match input {
        VerificationInput::Scalar(literal) => verify_scalar_builtin(builtin, literal),
        VerificationInput::TableColumn(column) => verify_column_builtin(builtin, column),
        VerificationInput::ParquetFile(path) if builtin == BuiltinType::Parquet => {
            let witness = TableWitness::from_parquet_file(path)?;
            Ok(ExecutionOutcome::verified(to_json(&witness)?))
        }
        _ => Err(VerificationError::InputKindMismatch {
            expected: match builtin {
                BuiltinType::Parquet => "parquet_file",
                BuiltinType::Bytes
                | BuiltinType::Utf8
                | BuiltinType::String
                | BuiltinType::Bool
                | BuiltinType::Int64
                | BuiltinType::Float64
                | BuiltinType::Date
                | BuiltinType::DateTime => "scalar or table_column_witness",
                BuiltinType::Json => "scalar",
            },
            actual: input.kind_name(),
        }),
    }
}

fn verify_scalar_builtin(
    builtin: BuiltinType,
    literal: &Literal,
) -> Result<ExecutionOutcome, VerificationError> {
    let verified = match builtin {
        BuiltinType::Bool => matches!(literal, Literal::Bool(_)),
        BuiltinType::Int64 => matches!(literal, Literal::Integer(_)),
        BuiltinType::Float64 => matches!(literal, Literal::Float(_)),
        BuiltinType::String | BuiltinType::Utf8 => matches!(literal, Literal::String(_)),
        BuiltinType::Bytes
        | BuiltinType::Json
        | BuiltinType::Parquet
        | BuiltinType::Date
        | BuiltinType::DateTime => {
            return Err(VerificationError::UnsupportedConstraint {
                constraint: builtin.as_str().to_string(),
                context: "scalar verification",
            });
        }
    };

    let evidence = json!({
        "kind": "scalar_builtin",
        "builtin": builtin.as_str(),
        "literal": literal,
    });

    Ok(if verified {
        ExecutionOutcome::verified(evidence)
    } else {
        ExecutionOutcome::rejected(
            format!("scalar does not satisfy builtin '{}'", builtin.as_str()),
            evidence,
        )
    })
}

fn verify_column_builtin(
    builtin: BuiltinType,
    column: &TableColumnWitness,
) -> Result<ExecutionOutcome, VerificationError> {
    let verified = match builtin {
        BuiltinType::Bytes => matches!(column.data_type.as_str(), "binary" | "large_binary"),
        BuiltinType::Utf8 | BuiltinType::String => {
            matches!(column.data_type.as_str(), "utf8" | "large_utf8")
        }
        BuiltinType::Bool => column.data_type == "bool",
        BuiltinType::Int64 => column.data_type == "int64",
        BuiltinType::Float64 => column.data_type == "float64",
        BuiltinType::Date => matches!(column.data_type.as_str(), "date32" | "date64"),
        BuiltinType::DateTime => column.data_type.starts_with("timestamp["),
        BuiltinType::Json | BuiltinType::Parquet => {
            return Err(VerificationError::UnsupportedConstraint {
                constraint: builtin.as_str().to_string(),
                context: "table column verification",
            });
        }
    };

    let evidence = json!({
        "kind": "table_column_builtin",
        "builtin": builtin.as_str(),
        "column": column,
    });

    Ok(if verified {
        ExecutionOutcome::verified(evidence)
    } else {
        ExecutionOutcome::rejected(
            format!(
                "column '{}' of type '{}' does not satisfy builtin '{}'",
                column.name,
                column.data_type,
                builtin.as_str()
            ),
            evidence,
        )
    })
}

fn verify_constructor(
    constructor: &ConstructorExpr,
    input: &VerificationInput,
) -> Result<ExecutionOutcome, VerificationError> {
    match constructor.name {
        BuiltinConstructor::Csv => match input {
            VerificationInput::Csv(witness) => verify_csv_constructor(constructor, witness),
            _ => Err(VerificationError::InputKindMismatch {
                expected: "csv_witness",
                actual: input.kind_name(),
            }),
        },
        BuiltinConstructor::Min | BuiltinConstructor::Max | BuiltinConstructor::Enum => match input
        {
            VerificationInput::Scalar(literal) => verify_scalar_constructor(constructor, literal),
            _ => Err(VerificationError::InputKindMismatch {
                expected: "scalar",
                actual: input.kind_name(),
            }),
        },
        BuiltinConstructor::Nullable => match input {
            VerificationInput::TableColumn(column) => {
                let evidence = json!({ "kind": "nullable", "column": column });
                Ok(if column.nullable {
                    ExecutionOutcome::verified(evidence)
                } else {
                    ExecutionOutcome::rejected(
                        format!("column '{}' is not nullable", column.name),
                        evidence,
                    )
                })
            }
            _ => Err(VerificationError::InputKindMismatch {
                expected: "table_column_witness",
                actual: input.kind_name(),
            }),
        },
        BuiltinConstructor::Unit => Err(VerificationError::UnsupportedConstraint {
            constraint: constructor.name.as_str().to_string(),
            context: "unit verification without measurement metadata",
        }),
    }
}

fn verify_csv_constructor(
    constructor: &ConstructorExpr,
    witness: &CsvWitness,
) -> Result<ExecutionOutcome, VerificationError> {
    let mut diagnostics = Vec::new();
    let mut verified = true;

    if let Some(Literal::String(delimiter)) = constructor.args.get("delimiter") {
        if &witness.delimiter != delimiter {
            verified = false;
            diagnostics.push(format!(
                "expected delimiter '{}', got '{}'",
                delimiter, witness.delimiter
            ));
        }
    }

    if let Some(Literal::Bool(header)) = constructor.args.get("header") {
        if witness.header != *header {
            verified = false;
            diagnostics.push(format!(
                "expected header={}, got header={}",
                header, witness.header
            ));
        }
    }

    let evidence = to_json(witness)?;
    Ok(if verified {
        ExecutionOutcome::verified(evidence)
    } else {
        ExecutionOutcome {
            verified: false,
            diagnostics,
            evidence,
        }
    })
}

fn verify_scalar_constructor(
    constructor: &ConstructorExpr,
    literal: &Literal,
) -> Result<ExecutionOutcome, VerificationError> {
    let evidence = json!({
        "kind": "scalar_constructor",
        "constructor": constructor.name.as_str(),
        "literal": literal,
    });

    let verified = match constructor.name {
        BuiltinConstructor::Min => {
            let actual =
                numeric_literal_value(literal).ok_or(VerificationError::InvalidConstructorArg {
                    constructor: constructor.name,
                    arg: "value",
                    expected: "a numeric scalar input",
                })?;
            let min = constructor_numeric_value(constructor, "value")?;
            actual >= min
        }
        BuiltinConstructor::Max => {
            let actual =
                numeric_literal_value(literal).ok_or(VerificationError::InvalidConstructorArg {
                    constructor: constructor.name,
                    arg: "value",
                    expected: "a numeric scalar input",
                })?;
            let max = constructor_numeric_value(constructor, "value")?;
            actual <= max
        }
        BuiltinConstructor::Enum => list_arg(constructor, "values")?.contains(literal),
        other => {
            return Err(VerificationError::UnsupportedConstraint {
                constraint: other.as_str().to_string(),
                context: "scalar constructor verification",
            });
        }
    };

    Ok(if verified {
        ExecutionOutcome::verified(evidence)
    } else {
        ExecutionOutcome::rejected(
            format!(
                "scalar does not satisfy constructor '{}'",
                constructor.name.as_str()
            ),
            evidence,
        )
    })
}

fn verify_record(
    open: bool,
    fields: &[RecordFieldPlan],
    input: &VerificationInput,
) -> Result<ExecutionOutcome, VerificationError> {
    match input {
        VerificationInput::Record(values) => verify_record_map(open, fields, values),
        VerificationInput::Table(table) => verify_record_against_table(open, fields, table),
        _ => Err(VerificationError::InputKindMismatch {
            expected: "record or table_witness",
            actual: input.kind_name(),
        }),
    }
}

fn verify_record_map(
    open: bool,
    fields: &[RecordFieldPlan],
    values: &BTreeMap<String, VerificationInput>,
) -> Result<ExecutionOutcome, VerificationError> {
    let mut present_fields = Vec::new();
    let mut absent_optional_fields = Vec::new();
    let mut diagnostics = Vec::new();
    let mut child_evidence = BTreeMap::new();
    let mut verified = true;

    let expected_names = fields
        .iter()
        .map(|field| field.name.as_str())
        .collect::<BTreeSet<_>>();

    for field in fields {
        match values.get(&field.name) {
            Some(value) => {
                present_fields.push(field.name.clone());
                let outcome = execute_plan(&field.plan, value)?;
                if !outcome.verified {
                    verified = false;
                    diagnostics.extend(outcome.diagnostics.clone());
                }
                child_evidence.insert(field.name.clone(), outcome.evidence);
            }
            None if field.optional => absent_optional_fields.push(field.name.clone()),
            None => {
                verified = false;
                diagnostics.push(format!("missing required field '{}'", field.name));
            }
        }
    }

    if !open {
        for field_name in values.keys() {
            if !expected_names.contains(field_name.as_str()) {
                verified = false;
                diagnostics.push(format!("unexpected field '{}'", field_name));
            }
        }
    }

    let record_witness = RecordWitness {
        present_fields,
        absent_optional_fields,
    };
    let evidence = json!({
        "record": record_witness,
        "fields": child_evidence,
    });

    Ok(ExecutionOutcome {
        verified,
        diagnostics,
        evidence,
    })
}

fn verify_record_against_table(
    open: bool,
    fields: &[RecordFieldPlan],
    table: &TableWitness,
) -> Result<ExecutionOutcome, VerificationError> {
    let mut present_fields = Vec::new();
    let mut absent_optional_fields = Vec::new();
    let mut diagnostics = Vec::new();
    let mut child_evidence = BTreeMap::new();
    let mut verified = true;

    let columns = table
        .columns
        .iter()
        .map(|column| (column.name.as_str(), column))
        .collect::<BTreeMap<_, _>>();
    let expected_names = fields
        .iter()
        .map(|field| field.name.as_str())
        .collect::<BTreeSet<_>>();

    for field in fields {
        match columns.get(field.name.as_str()) {
            Some(column) => {
                present_fields.push(field.name.clone());
                let outcome = execute_plan(
                    &field.plan,
                    &VerificationInput::TableColumn((*column).clone()),
                )?;
                if !outcome.verified {
                    verified = false;
                    diagnostics.extend(outcome.diagnostics.clone());
                }
                child_evidence.insert(field.name.clone(), outcome.evidence);
            }
            None if field.optional => absent_optional_fields.push(field.name.clone()),
            None => {
                verified = false;
                diagnostics.push(format!("missing required column '{}'", field.name));
            }
        }
    }

    if !open {
        for column in &table.columns {
            if !expected_names.contains(column.name.as_str()) {
                verified = false;
                diagnostics.push(format!("unexpected column '{}'", column.name));
            }
        }
    }

    let record_witness = RecordWitness {
        present_fields,
        absent_optional_fields,
    };
    let evidence = json!({
        "record": record_witness,
        "table": table,
        "fields": child_evidence,
    });

    Ok(ExecutionOutcome {
        verified,
        diagnostics,
        evidence,
    })
}

fn verify_collection(
    item_plan: &VerificationPlan,
    input: &VerificationInput,
) -> Result<ExecutionOutcome, VerificationError> {
    match input {
        VerificationInput::Collection(items) => {
            let mut outcomes = Vec::with_capacity(items.len());
            for item in items {
                outcomes.push(execute_plan(item_plan, item)?);
            }

            Ok(ExecutionOutcome::merge_all(outcomes))
        }
        _ => Err(VerificationError::InputKindMismatch {
            expected: "collection",
            actual: input.kind_name(),
        }),
    }
}

fn verify_table(
    row_plan: &VerificationPlan,
    input: &VerificationInput,
) -> Result<ExecutionOutcome, VerificationError> {
    let table = match input {
        VerificationInput::Table(table) => table.clone(),
        VerificationInput::ParquetFile(path) => TableWitness::from_parquet_file(path)?,
        _ => {
            return Err(VerificationError::InputKindMismatch {
                expected: "table_witness or parquet_file",
                actual: input.kind_name(),
            });
        }
    };

    let row_outcome = execute_plan(row_plan, &VerificationInput::Table(table.clone()))?;
    let evidence = json!({
        "table": table,
        "row": row_outcome.evidence,
    });

    Ok(ExecutionOutcome {
        verified: row_outcome.verified,
        diagnostics: row_outcome.diagnostics,
        evidence,
    })
}

fn constructor_numeric_value(
    constructor: &ConstructorExpr,
    name: &'static str,
) -> Result<f64, VerificationError> {
    let value = constructor
        .args
        .get(name)
        .ok_or(VerificationError::MissingConstructorArg {
            constructor: constructor.name,
            arg: name,
        })?;

    numeric_literal_value(value).ok_or(VerificationError::InvalidConstructorArg {
        constructor: constructor.name,
        arg: name,
        expected: "an integer or float literal",
    })
}

fn numeric_literal_value(literal: &Literal) -> Option<f64> {
    match literal {
        Literal::Integer(value) => Some(*value as f64),
        Literal::Float(value) => Some(value.into_inner()),
        _ => None,
    }
}

fn list_arg<'a>(
    constructor: &'a ConstructorExpr,
    name: &'static str,
) -> Result<&'a [Literal], VerificationError> {
    let value = constructor
        .args
        .get(name)
        .ok_or(VerificationError::MissingConstructorArg {
            constructor: constructor.name,
            arg: name,
        })?;

    match value {
        Literal::List(values) if !values.is_empty() => Ok(values.as_slice()),
        _ => Err(VerificationError::InvalidConstructorArg {
            constructor: constructor.name,
            arg: name,
            expected: "a non-empty list of scalar literals",
        }),
    }
}

fn to_json<T: Serialize>(value: &T) -> Result<Value, VerificationError> {
    serde_json::to_value(value).map_err(VerificationError::EvidenceEncodingFailed)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::registry::{TypeRegistry, TypeVersion};
    use crate::syntax::{RecordExpr, RecordField, TypeDefinition};

    #[test]
    fn verification_report_can_carry_diagnostics() {
        let report = VerificationReport {
            verifier: "builtin.csv".to_string(),
            verdict: VerificationVerdict::Verified,
            diagnostics: vec!["parsed successfully".to_string()],
            evidence: None,
        };

        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(report.verdict, VerificationVerdict::Verified);
    }

    #[test]
    fn registry_compiles_csv_and_table_plan() {
        let mut defs = TypeDefinitions::default();
        let published = TypeRegistry::default();
        defs.insert(TypeDefinition::new(
            "Row",
            TypeExpr::Record(RecordExpr {
                fields: vec![RecordField {
                    name: "species".to_string(),
                    ty: TypeExpr::ref_("string"),
                    optional: false,
                }],
                open: false,
            }),
        ))
        .expect("insert row");

        let registry = BuiltinVerifierRegistry;
        let plan = registry
            .compile(
                &defs,
                &published,
                &TypeExpr::intersection(vec![
                    TypeExpr::Constructor(ConstructorExpr {
                        name: BuiltinConstructor::Csv,
                        args: BTreeMap::from([("header".to_string(), Literal::Bool(true))]),
                    }),
                    TypeExpr::Table(Box::new(TypeExpr::named_ref("Row"))),
                ])
                .expect("non-empty intersection"),
            )
            .expect("compile verification plan");

        match plan {
            VerificationPlan::All(parts) => assert_eq!(parts.len(), 2),
            other => panic!("expected intersection plan, got {other:?}"),
        }
    }

    #[test]
    fn csv_witness_verification_rejects_mismatched_header() {
        let defs = TypeDefinitions::default();
        let published = TypeRegistry::default();
        let registry = BuiltinVerifierRegistry;
        let report = registry
            .verify(
                &defs,
                &published,
                &TypeExpr::Constructor(ConstructorExpr {
                    name: BuiltinConstructor::Csv,
                    args: BTreeMap::from([("header".to_string(), Literal::Bool(true))]),
                }),
                &VerificationInput::Csv(CsvWitness {
                    delimiter: ",".to_string(),
                    header: false,
                    columns: vec!["species".to_string()],
                    row_count: Some(1),
                }),
            )
            .expect("verification should run");

        assert_eq!(report.verdict, VerificationVerdict::Rejected);
        assert_eq!(report.diagnostics.len(), 1);
    }

    #[test]
    fn table_verification_checks_required_columns_and_types() {
        let defs = TypeDefinitions::default();
        let published = TypeRegistry::default();
        let registry = BuiltinVerifierRegistry;
        let report = registry
            .verify(
                &defs,
                &published,
                &TypeExpr::Table(Box::new(TypeExpr::Record(RecordExpr {
                    fields: vec![
                        RecordField {
                            name: "species".to_string(),
                            ty: TypeExpr::ref_("string"),
                            optional: false,
                        },
                        RecordField {
                            name: "wp".to_string(),
                            ty: TypeExpr::ref_("float64"),
                            optional: false,
                        },
                    ],
                    open: false,
                }))),
                &VerificationInput::Table(TableWitness {
                    columns: vec![
                        TableColumnWitness {
                            name: "species".to_string(),
                            data_type: "utf8".to_string(),
                            nullable: false,
                        },
                        TableColumnWitness {
                            name: "wp".to_string(),
                            data_type: "float64".to_string(),
                            nullable: false,
                        },
                    ],
                    row_count: Some(10),
                }),
            )
            .expect("verification should run");

        assert_eq!(report.verdict, VerificationVerdict::Verified);
    }

    #[test]
    fn table_verification_rejects_missing_required_column() {
        let defs = TypeDefinitions::default();
        let published = TypeRegistry::default();
        let registry = BuiltinVerifierRegistry;
        let report = registry
            .verify(
                &defs,
                &published,
                &TypeExpr::Table(Box::new(TypeExpr::Record(RecordExpr {
                    fields: vec![RecordField {
                        name: "wp".to_string(),
                        ty: TypeExpr::ref_("float64"),
                        optional: false,
                    }],
                    open: false,
                }))),
                &VerificationInput::Table(TableWitness {
                    columns: vec![TableColumnWitness {
                        name: "species".to_string(),
                        data_type: "utf8".to_string(),
                        nullable: false,
                    }],
                    row_count: Some(10),
                }),
            )
            .expect("verification should run");

        assert_eq!(report.verdict, VerificationVerdict::Rejected);
        assert_eq!(report.diagnostics[0], "missing required column 'wp'");
    }

    #[test]
    fn scalar_min_verification_rejects_values_below_bound() {
        let defs = TypeDefinitions::default();
        let published = TypeRegistry::default();
        let registry = BuiltinVerifierRegistry;
        let report = registry
            .verify(
                &defs,
                &published,
                &TypeExpr::Constructor(ConstructorExpr {
                    name: BuiltinConstructor::Min,
                    args: BTreeMap::from([("value".to_string(), Literal::Integer(5))]),
                }),
                &VerificationInput::Scalar(Literal::Integer(2)),
            )
            .expect("verification should run");

        assert_eq!(report.verdict, VerificationVerdict::Rejected);
    }

    #[test]
    fn malformed_min_constructor_errors_instead_of_falling_back() {
        let defs = TypeDefinitions::default();
        let published = TypeRegistry::default();
        let registry = BuiltinVerifierRegistry;
        let err = registry
            .verify(
                &defs,
                &published,
                &TypeExpr::Constructor(ConstructorExpr {
                    name: BuiltinConstructor::Min,
                    args: BTreeMap::from([(
                        "value".to_string(),
                        Literal::String("oops".to_string()),
                    )]),
                }),
                &VerificationInput::Scalar(Literal::Integer(2)),
            )
            .expect_err("malformed constructor args should error");

        assert!(matches!(
            err,
            VerificationError::Canonicalization(_)
                | VerificationError::InvalidConstructorArg { .. }
        ));
    }

    #[test]
    fn derived_inputs_can_satisfy_conjunctive_csv_and_table_types() {
        let mut defs = TypeDefinitions::default();
        let published = TypeRegistry::default();
        defs.insert(TypeDefinition::new(
            "Row",
            TypeExpr::Record(RecordExpr {
                fields: vec![
                    RecordField {
                        name: "species".to_string(),
                        ty: TypeExpr::ref_("string"),
                        optional: false,
                    },
                    RecordField {
                        name: "wp".to_string(),
                        ty: TypeExpr::ref_("float64"),
                        optional: false,
                    },
                ],
                open: false,
            }),
        ))
        .expect("insert row");

        let registry = BuiltinVerifierRegistry;
        let report = registry
            .verify(
                &defs,
                &published,
                &TypeExpr::intersection(vec![
                    TypeExpr::Constructor(ConstructorExpr {
                        name: BuiltinConstructor::Csv,
                        args: BTreeMap::from([("header".to_string(), Literal::Bool(true))]),
                    }),
                    TypeExpr::Table(Box::new(TypeExpr::named_ref("Row"))),
                ])
                .expect("non-empty intersection"),
                &VerificationInput::Derived(vec![
                    VerificationInput::Csv(CsvWitness {
                        delimiter: ",".to_string(),
                        header: true,
                        columns: vec!["species".to_string(), "wp".to_string()],
                        row_count: Some(10),
                    }),
                    VerificationInput::Table(TableWitness {
                        columns: vec![
                            TableColumnWitness {
                                name: "species".to_string(),
                                data_type: "utf8".to_string(),
                                nullable: false,
                            },
                            TableColumnWitness {
                                name: "wp".to_string(),
                                data_type: "float64".to_string(),
                                nullable: false,
                            },
                        ],
                        row_count: Some(10),
                    }),
                ]),
            )
            .expect("verification should run");

        assert_eq!(report.verdict, VerificationVerdict::Verified);
    }

    #[test]
    fn versioned_refs_are_verified_via_the_registry() {
        let defs = TypeDefinitions::default();
        let mut published = TypeRegistry::default();
        published
            .insert(TypeVersion::new(
                "std/WaterPotentialTable",
                "1",
                TypeExpr::Table(Box::new(TypeExpr::Record(RecordExpr {
                    fields: vec![
                        RecordField {
                            name: "species".to_string(),
                            ty: TypeExpr::ref_("string"),
                            optional: false,
                        },
                        RecordField {
                            name: "wp".to_string(),
                            ty: TypeExpr::ref_("float64"),
                            optional: false,
                        },
                    ],
                    open: false,
                }))),
            ))
            .expect("insert published type");

        let registry = BuiltinVerifierRegistry;
        let report = registry
            .verify(
                &defs,
                &published,
                &TypeExpr::Ref(TypeRefExpr::new(
                    "std/WaterPotentialTable",
                    Some("1".to_string()),
                )),
                &VerificationInput::Table(TableWitness {
                    columns: vec![
                        TableColumnWitness {
                            name: "species".to_string(),
                            data_type: "utf8".to_string(),
                            nullable: false,
                        },
                        TableColumnWitness {
                            name: "wp".to_string(),
                            data_type: "float64".to_string(),
                            nullable: false,
                        },
                    ],
                    row_count: Some(10),
                }),
            )
            .expect("verification should run");

        assert_eq!(report.verdict, VerificationVerdict::Verified);
    }
}
