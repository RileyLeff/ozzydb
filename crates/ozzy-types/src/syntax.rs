//! Surface syntax AST for the v4 type language.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Builtin leaf types in the v1 OzzyDB type language.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum BuiltinType {
    Bytes,
    Utf8,
    Json,
    Parquet,
    String,
    Bool,
    Int64,
    Float64,
    Date,
    #[serde(rename = "datetime")]
    DateTime,
}

impl BuiltinType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Bytes => "bytes",
            Self::Utf8 => "utf8",
            Self::Json => "json",
            Self::Parquet => "parquet",
            Self::String => "string",
            Self::Bool => "bool",
            Self::Int64 => "int64",
            Self::Float64 => "float64",
            Self::Date => "date",
            Self::DateTime => "datetime",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "bytes" => Some(Self::Bytes),
            "utf8" => Some(Self::Utf8),
            "json" => Some(Self::Json),
            "parquet" => Some(Self::Parquet),
            "string" => Some(Self::String),
            "bool" => Some(Self::Bool),
            "int64" => Some(Self::Int64),
            "float64" => Some(Self::Float64),
            "date" => Some(Self::Date),
            "datetime" => Some(Self::DateTime),
            _ => None,
        }
    }
}

impl fmt::Display for BuiltinType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Builtin constructors in the v1 OzzyDB type language.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum BuiltinConstructor {
    Csv,
    Unit,
    Min,
    Max,
    Enum,
    Nullable,
}

impl BuiltinConstructor {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Unit => "unit",
            Self::Min => "min",
            Self::Max => "max",
            Self::Enum => "enum",
            Self::Nullable => "nullable",
        }
    }
}

impl fmt::Display for BuiltinConstructor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A reference to a named type, optionally pinned to a published version.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TypeRefExpr {
    pub name: String,
    pub version: Option<String>,
}

impl TypeRefExpr {
    pub fn new(name: impl Into<String>, version: Option<String>) -> Self {
        Self {
            name: name.into(),
            version,
        }
    }
}

/// Literal values accepted by constructor arguments.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Literal {
    Bool(bool),
    Integer(i64),
    Float(OrderedFloat<f64>),
    String(String),
    List(Vec<Literal>),
}

/// A constructor application like `csv(delimiter=",", header=true)`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConstructorExpr {
    pub name: BuiltinConstructor,
    #[serde(default)]
    pub args: BTreeMap<String, Literal>,
}

/// A single record field in a record type expression.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecordField {
    pub name: String,
    pub ty: TypeExpr,
    #[serde(default)]
    pub optional: bool,
}

/// A record expression. Records are closed by default; `open=true` models `...`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecordExpr {
    #[serde(default)]
    pub fields: Vec<RecordField>,
    #[serde(default)]
    pub open: bool,
}

/// The v1 surface syntax tree for OzzyDB type expressions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TypeExpr {
    Builtin(BuiltinType),
    Ref(TypeRefExpr),
    Intersection(Vec<TypeExpr>),
    Constructor(ConstructorExpr),
    Record(RecordExpr),
    Collection(Box<TypeExpr>),
    Table(Box<TypeExpr>),
    Never,
}

impl TypeExpr {
    pub fn builtin(builtin: BuiltinType) -> Self {
        Self::Builtin(builtin)
    }

    pub fn named_ref(name: impl Into<String>) -> Self {
        Self::Ref(TypeRefExpr::new(name, None))
    }

    pub fn ref_(name: impl Into<String>) -> Self {
        let name = name.into();
        match BuiltinType::parse(&name) {
            Some(builtin) => Self::Builtin(builtin),
            None => Self::Ref(TypeRefExpr::new(name, None)),
        }
    }

    pub fn intersection(parts: Vec<TypeExpr>) -> Result<Self, TypeLanguageError> {
        if parts.is_empty() {
            return Err(TypeLanguageError::EmptyIntersection);
        }

        Ok(Self::Intersection(parts))
    }
}

/// A local named type definition used before publication.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct TypeDefinition {
    pub name: String,
    pub expr: TypeExpr,
}

impl TypeDefinition {
    pub fn new(name: impl Into<String>, expr: TypeExpr) -> Self {
        Self {
            name: name.into(),
            expr,
        }
    }
}

/// Errors in the surface type language should be explicit and non-lossy.
#[derive(Debug, Error, PartialEq)]
pub enum TypeLanguageError {
    #[error("type definition '{name}' already exists")]
    DuplicateTypeDefinition { name: String },
    #[error("type reference '{name}' is unknown in the current local definition set")]
    UnknownTypeReference { name: String },
    #[error("builtin type '{name}' cannot be version-pinned as '{version}'")]
    BuiltinTypeCannotHaveVersion { name: String, version: String },
    #[error("intersection must contain at least one expression")]
    EmptyIntersection,
    #[error("record contains duplicate field '{field}'")]
    DuplicateRecordField { field: String },
    #[error("constructor '{constructor}' does not accept argument '{arg}'")]
    UnknownConstructorArg {
        constructor: BuiltinConstructor,
        arg: String,
    },
    #[error("constructor '{constructor}' requires argument '{arg}'")]
    MissingConstructorArg {
        constructor: BuiltinConstructor,
        arg: String,
    },
    #[error("constructor '{constructor}' expects {expected} for argument '{arg}'")]
    InvalidConstructorArg {
        constructor: BuiltinConstructor,
        arg: String,
        expected: &'static str,
    },
}

/// A local set of named type definitions. This is the v1 surface layer that
/// later compiles into published `TypeVersion`s.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct TypeDefinitions {
    pub types: BTreeMap<String, TypeDefinition>,
}

impl TypeDefinitions {
    pub fn insert(&mut self, definition: TypeDefinition) -> Result<(), TypeLanguageError> {
        if self.types.contains_key(&definition.name) {
            return Err(TypeLanguageError::DuplicateTypeDefinition {
                name: definition.name,
            });
        }

        self.types.insert(definition.name.clone(), definition);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&TypeDefinition> {
        self.types.get(name)
    }

    pub fn validate_all(&self) -> Result<(), TypeLanguageError> {
        for definition in self.types.values() {
            self.validate_expr(&definition.expr)?;
        }

        Ok(())
    }

    pub fn validate_expr(&self, expr: &TypeExpr) -> Result<(), TypeLanguageError> {
        match expr {
            TypeExpr::Builtin(_) | TypeExpr::Never => Ok(()),
            TypeExpr::Ref(type_ref) => self.validate_ref(type_ref),
            TypeExpr::Intersection(parts) => {
                if parts.is_empty() {
                    return Err(TypeLanguageError::EmptyIntersection);
                }

                for part in parts {
                    self.validate_expr(part)?;
                }
                Ok(())
            }
            TypeExpr::Constructor(constructor) => validate_constructor(constructor),
            TypeExpr::Record(record) => self.validate_record(record),
            TypeExpr::Collection(item) => self.validate_expr(item),
            TypeExpr::Table(row) => self.validate_expr(row),
        }
    }

    fn validate_ref(&self, type_ref: &TypeRefExpr) -> Result<(), TypeLanguageError> {
        if let Some(builtin) = BuiltinType::parse(&type_ref.name) {
            if let Some(version) = &type_ref.version {
                return Err(TypeLanguageError::BuiltinTypeCannotHaveVersion {
                    name: builtin.as_str().to_string(),
                    version: version.clone(),
                });
            }

            return Ok(());
        }

        if type_ref.version.is_some() {
            return Ok(());
        }

        if self.types.contains_key(&type_ref.name) {
            return Ok(());
        }

        Err(TypeLanguageError::UnknownTypeReference {
            name: type_ref.name.clone(),
        })
    }

    fn validate_record(&self, record: &RecordExpr) -> Result<(), TypeLanguageError> {
        let mut seen_fields = BTreeSet::new();

        for field in &record.fields {
            if !seen_fields.insert(field.name.clone()) {
                return Err(TypeLanguageError::DuplicateRecordField {
                    field: field.name.clone(),
                });
            }

            self.validate_expr(&field.ty)?;
        }

        Ok(())
    }
}

fn validate_constructor(constructor: &ConstructorExpr) -> Result<(), TypeLanguageError> {
    match constructor.name {
        BuiltinConstructor::Csv => {
            validate_only_allowed_args(constructor, &["delimiter", "header"])?;

            if let Some(value) = constructor.args.get("delimiter") {
                require_literal_kind(constructor.name, "delimiter", value, LiteralKind::String)?;
            }
            if let Some(value) = constructor.args.get("header") {
                require_literal_kind(constructor.name, "header", value, LiteralKind::Bool)?;
            }

            Ok(())
        }
        BuiltinConstructor::Unit => {
            validate_only_allowed_args(constructor, &["value"])?;
            let value = required_arg(constructor, "value")?;
            require_literal_kind(constructor.name, "value", value, LiteralKind::String)
        }
        BuiltinConstructor::Min | BuiltinConstructor::Max => {
            validate_only_allowed_args(constructor, &["value"])?;
            let value = required_arg(constructor, "value")?;
            require_literal_kind(constructor.name, "value", value, LiteralKind::Number)
        }
        BuiltinConstructor::Enum => {
            validate_only_allowed_args(constructor, &["values"])?;
            let value = required_arg(constructor, "values")?;
            require_literal_kind(constructor.name, "values", value, LiteralKind::ScalarList)
        }
        BuiltinConstructor::Nullable => {
            validate_only_allowed_args(constructor, &[])?;
            Ok(())
        }
    }
}

fn validate_only_allowed_args(
    constructor: &ConstructorExpr,
    allowed: &[&str],
) -> Result<(), TypeLanguageError> {
    for arg in constructor.args.keys() {
        if !allowed.contains(&arg.as_str()) {
            return Err(TypeLanguageError::UnknownConstructorArg {
                constructor: constructor.name,
                arg: arg.clone(),
            });
        }
    }

    Ok(())
}

fn required_arg<'a>(
    constructor: &'a ConstructorExpr,
    arg: &'static str,
) -> Result<&'a Literal, TypeLanguageError> {
    constructor
        .args
        .get(arg)
        .ok_or_else(|| TypeLanguageError::MissingConstructorArg {
            constructor: constructor.name,
            arg: arg.to_string(),
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LiteralKind {
    Bool,
    String,
    Number,
    ScalarList,
}

impl LiteralKind {
    fn description(self) -> &'static str {
        match self {
            Self::Bool => "a boolean literal",
            Self::String => "a string literal",
            Self::Number => "an integer or float literal",
            Self::ScalarList => "a non-empty list of scalar literals",
        }
    }
}

fn require_literal_kind(
    constructor: BuiltinConstructor,
    arg: &'static str,
    literal: &Literal,
    expected: LiteralKind,
) -> Result<(), TypeLanguageError> {
    let matches = match expected {
        LiteralKind::Bool => matches!(literal, Literal::Bool(_)),
        LiteralKind::String => matches!(literal, Literal::String(_)),
        LiteralKind::Number => {
            matches!(literal, Literal::Integer(_))
                || matches!(literal, Literal::Float(value) if value.is_finite())
        }
        LiteralKind::ScalarList => match literal {
            Literal::List(values) => !values.is_empty() && values.iter().all(Literal::is_scalar),
            _ => false,
        },
    };

    if matches {
        return Ok(());
    }

    Err(TypeLanguageError::InvalidConstructorArg {
        constructor,
        arg: arg.to_string(),
        expected: expected.description(),
    })
}

impl Literal {
    fn is_scalar(&self) -> bool {
        matches!(
            self,
            Literal::Bool(_) | Literal::Integer(_) | Literal::String(_)
        ) || matches!(self, Literal::Float(value) if value.is_finite())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_build_open_record_expression() {
        let expr = TypeExpr::Record(RecordExpr {
            fields: vec![RecordField {
                name: "site_id".to_string(),
                ty: TypeExpr::ref_("string"),
                optional: false,
            }],
            open: true,
        });

        match expr {
            TypeExpr::Record(record) => {
                assert!(record.open);
                assert_eq!(record.fields.len(), 1);
                assert_eq!(record.fields[0].name, "site_id");
            }
            other => panic!("expected record expression, got {other:?}"),
        }
    }

    #[test]
    fn intersection_helper_rejects_empty_inputs() {
        let err = TypeExpr::intersection(vec![]).expect_err("empty intersections should fail");
        assert_eq!(err, TypeLanguageError::EmptyIntersection);
    }

    #[test]
    fn builtin_names_are_promoted_out_of_named_refs() {
        assert_eq!(
            TypeExpr::ref_("float64"),
            TypeExpr::Builtin(BuiltinType::Float64)
        );
        assert_eq!(TypeExpr::ref_("date"), TypeExpr::Builtin(BuiltinType::Date));
        assert_eq!(
            TypeExpr::ref_("WaterPotential"),
            TypeExpr::Ref(TypeRefExpr::new("WaterPotential", None))
        );
    }

    #[test]
    fn duplicate_type_definition_names_are_rejected() {
        let mut defs = TypeDefinitions::default();
        defs.insert(TypeDefinition::new(
            "WaterPotential",
            TypeExpr::ref_("float64"),
        ))
        .expect("first insert should succeed");

        let err = defs
            .insert(TypeDefinition::new(
                "WaterPotential",
                TypeExpr::ref_("float64"),
            ))
            .expect_err("duplicate type names should fail");
        assert_eq!(
            err,
            TypeLanguageError::DuplicateTypeDefinition {
                name: "WaterPotential".to_string(),
            }
        );
    }

    #[test]
    fn local_named_aliases_validate_successfully() {
        let mut defs = TypeDefinitions::default();
        defs.insert(TypeDefinition::new(
            "WaterPotential",
            TypeExpr::intersection(vec![
                TypeExpr::ref_("float64"),
                TypeExpr::Constructor(ConstructorExpr {
                    name: BuiltinConstructor::Unit,
                    args: BTreeMap::from([(
                        "value".to_string(),
                        Literal::String("MPa".to_string()),
                    )]),
                }),
            ])
            .expect("non-empty intersection"),
        ))
        .expect("insert WaterPotential");
        defs.insert(TypeDefinition::new(
            "WaterPotentialRow",
            TypeExpr::Record(RecordExpr {
                fields: vec![RecordField {
                    name: "wp".to_string(),
                    ty: TypeExpr::named_ref("WaterPotential"),
                    optional: false,
                }],
                open: false,
            }),
        ))
        .expect("insert WaterPotentialRow");
        defs.insert(TypeDefinition::new(
            "WaterPotentialTable",
            TypeExpr::Table(Box::new(TypeExpr::named_ref("WaterPotentialRow"))),
        ))
        .expect("insert WaterPotentialTable");

        defs.validate_all().expect("definitions should validate");
    }

    #[test]
    fn unknown_unversioned_refs_are_rejected() {
        let defs = TypeDefinitions::default();

        let err = defs
            .validate_expr(&TypeExpr::named_ref("MissingType"))
            .expect_err("missing local alias should fail");
        assert_eq!(
            err,
            TypeLanguageError::UnknownTypeReference {
                name: "MissingType".to_string(),
            }
        );
    }

    #[test]
    fn versioned_refs_are_allowed_without_local_definition() {
        let defs = TypeDefinitions::default();

        defs.validate_expr(&TypeExpr::Ref(TypeRefExpr::new(
            "std/WaterPotential",
            Some("1".to_string()),
        )))
        .expect("external versioned refs are allowed in local validation");
    }

    #[test]
    fn builtin_types_cannot_be_version_pinned() {
        let defs = TypeDefinitions::default();

        let err = defs
            .validate_expr(&TypeExpr::Ref(TypeRefExpr::new(
                "float64",
                Some("1".to_string()),
            )))
            .expect_err("builtin refs should not accept versions");
        assert_eq!(
            err,
            TypeLanguageError::BuiltinTypeCannotHaveVersion {
                name: "float64".to_string(),
                version: "1".to_string(),
            }
        );
    }

    #[test]
    fn duplicate_record_fields_are_rejected() {
        let defs = TypeDefinitions::default();
        let record = TypeExpr::Record(RecordExpr {
            fields: vec![
                RecordField {
                    name: "site_id".to_string(),
                    ty: TypeExpr::ref_("string"),
                    optional: false,
                },
                RecordField {
                    name: "site_id".to_string(),
                    ty: TypeExpr::ref_("string"),
                    optional: true,
                },
            ],
            open: false,
        });

        let err = defs
            .validate_expr(&record)
            .expect_err("duplicate fields should fail");
        assert_eq!(
            err,
            TypeLanguageError::DuplicateRecordField {
                field: "site_id".to_string(),
            }
        );
    }

    #[test]
    fn csv_constructor_rejects_unknown_args() {
        let defs = TypeDefinitions::default();
        let expr = TypeExpr::Constructor(ConstructorExpr {
            name: BuiltinConstructor::Csv,
            args: BTreeMap::from([("mode".to_string(), Literal::String("strict".to_string()))]),
        });

        let err = defs
            .validate_expr(&expr)
            .expect_err("unknown constructor args should fail");
        assert_eq!(
            err,
            TypeLanguageError::UnknownConstructorArg {
                constructor: BuiltinConstructor::Csv,
                arg: "mode".to_string(),
            }
        );
    }

    #[test]
    fn enum_constructor_requires_non_empty_scalar_list() {
        let defs = TypeDefinitions::default();
        let expr = TypeExpr::Constructor(ConstructorExpr {
            name: BuiltinConstructor::Enum,
            args: BTreeMap::from([("values".to_string(), Literal::List(vec![]))]),
        });

        let err = defs
            .validate_expr(&expr)
            .expect_err("enum must receive a non-empty scalar list");
        assert_eq!(
            err,
            TypeLanguageError::InvalidConstructorArg {
                constructor: BuiltinConstructor::Enum,
                arg: "values".to_string(),
                expected: "a non-empty list of scalar literals",
            }
        );
    }
}
