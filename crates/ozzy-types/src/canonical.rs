//! Canonical type identifiers, normalization, and interning.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use blake3::hash;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::syntax::{
    BuiltinConstructor, BuiltinType, ConstructorExpr, Literal, RecordExpr, RecordField,
    TypeDefinitions, TypeExpr, TypeLanguageError, TypeRefExpr,
};

/// Stable identifier for a canonicalized type node.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CanonicalTypeId(String);

impl CanonicalTypeId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn from_expr(expr: &TypeExpr) -> Result<Self, CanonicalizationError> {
        let encoded = fingerprint_expr(expr);
        Ok(Self(format!("type_{}", hash(encoded.as_bytes()).to_hex())))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A canonical type node stored in the registry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CanonicalType {
    pub id: CanonicalTypeId,
    pub expr: TypeExpr,
}

impl CanonicalType {
    pub fn new(expr: TypeExpr) -> Result<Self, CanonicalizationError> {
        let id = CanonicalTypeId::from_expr(&expr)?;
        Ok(Self { id, expr })
    }

    pub fn canonicalize(
        defs: &TypeDefinitions,
        expr: &TypeExpr,
    ) -> Result<Self, CanonicalizationError> {
        let expr = canonicalize(defs, expr)?;
        Self::new(expr)
    }
}

/// Canonicalization must fail explicitly on malformed or cyclic input.
#[derive(Debug, Error)]
pub enum CanonicalizationError {
    #[error(transparent)]
    InvalidSurfaceType(#[from] TypeLanguageError),
    #[error("type definition cycle detected: {cycle:?}")]
    RecursiveTypeDefinition { cycle: Vec<String> },
    #[error("local type reference '{name}' could not be resolved during canonicalization")]
    UnresolvedLocalTypeReference { name: String },
}

/// A minimal in-memory canonical interner for Phase 1.
#[derive(Debug, Default)]
pub struct CanonicalTypeInterner {
    by_expr: BTreeMap<TypeExpr, CanonicalTypeId>,
    by_id: BTreeMap<CanonicalTypeId, CanonicalType>,
}

impl CanonicalTypeInterner {
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn intern(
        &mut self,
        defs: &TypeDefinitions,
        expr: &TypeExpr,
    ) -> Result<CanonicalType, CanonicalizationError> {
        let canonical = CanonicalType::canonicalize(defs, expr)?;

        if let Some(id) = self.by_expr.get(&canonical.expr) {
            return Ok(self
                .by_id
                .get(id)
                .expect("canonical interner should stay internally consistent")
                .clone());
        }

        self.by_expr
            .insert(canonical.expr.clone(), canonical.id.clone());
        self.by_id.insert(canonical.id.clone(), canonical.clone());
        Ok(canonical)
    }

    pub fn get(&self, id: &CanonicalTypeId) -> Option<&CanonicalType> {
        self.by_id.get(id)
    }
}

pub fn canonicalize(
    defs: &TypeDefinitions,
    expr: &TypeExpr,
) -> Result<TypeExpr, CanonicalizationError> {
    defs.validate_expr(expr)?;
    canonicalize_inner(defs, expr, &mut Vec::new())
}

fn canonicalize_inner(
    defs: &TypeDefinitions,
    expr: &TypeExpr,
    stack: &mut Vec<String>,
) -> Result<TypeExpr, CanonicalizationError> {
    match expr {
        TypeExpr::Builtin(_) | TypeExpr::Never => Ok(expr.clone()),
        TypeExpr::Ref(type_ref) => canonicalize_ref(defs, type_ref, stack),
        TypeExpr::Intersection(parts) => canonicalize_intersection(defs, parts, stack),
        TypeExpr::Constructor(constructor) => canonicalize_constructor(constructor),
        TypeExpr::Record(record) => canonicalize_record(defs, record, stack),
        TypeExpr::Collection(item) => Ok(TypeExpr::Collection(Box::new(canonicalize_inner(
            defs, item, stack,
        )?))),
        TypeExpr::Table(row) => Ok(TypeExpr::Table(Box::new(canonicalize_inner(
            defs, row, stack,
        )?))),
    }
}

fn canonicalize_ref(
    defs: &TypeDefinitions,
    type_ref: &TypeRefExpr,
    stack: &mut Vec<String>,
) -> Result<TypeExpr, CanonicalizationError> {
    if let Some(builtin) = BuiltinType::parse(&type_ref.name) {
        return Ok(TypeExpr::Builtin(builtin));
    }

    if type_ref.version.is_some() {
        return Ok(TypeExpr::Ref(type_ref.clone()));
    }

    if stack.contains(&type_ref.name) {
        let mut cycle = stack.clone();
        cycle.push(type_ref.name.clone());
        return Err(CanonicalizationError::RecursiveTypeDefinition { cycle });
    }

    let definition = defs.get(&type_ref.name).ok_or_else(|| {
        CanonicalizationError::UnresolvedLocalTypeReference {
            name: type_ref.name.clone(),
        }
    })?;
    defs.validate_expr(&definition.expr)?;

    stack.push(type_ref.name.clone());
    let result = canonicalize_inner(defs, &definition.expr, stack);
    stack.pop();
    result
}

fn canonicalize_record(
    defs: &TypeDefinitions,
    record: &RecordExpr,
    stack: &mut Vec<String>,
) -> Result<TypeExpr, CanonicalizationError> {
    let mut fields = Vec::with_capacity(record.fields.len());

    for field in &record.fields {
        let ty = canonicalize_inner(defs, &field.ty, stack)?;
        if ty == TypeExpr::Never {
            return Ok(TypeExpr::Never);
        }

        fields.push(RecordField {
            name: field.name.clone(),
            ty,
            optional: field.optional,
        });
    }

    fields.sort_by(|left, right| left.name.cmp(&right.name));

    Ok(TypeExpr::Record(RecordExpr {
        fields,
        open: record.open,
    }))
}

fn canonicalize_constructor(
    constructor: &ConstructorExpr,
) -> Result<TypeExpr, CanonicalizationError> {
    match constructor.name {
        BuiltinConstructor::Enum => {
            let values = list_arg(constructor, "values");
            let values = values
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();

            Ok(TypeExpr::Constructor(ConstructorExpr {
                name: BuiltinConstructor::Enum,
                args: BTreeMap::from([("values".to_string(), Literal::List(values))]),
            }))
        }
        _ => Ok(TypeExpr::Constructor(constructor.clone())),
    }
}

fn canonicalize_intersection(
    defs: &TypeDefinitions,
    parts: &[TypeExpr],
    stack: &mut Vec<String>,
) -> Result<TypeExpr, CanonicalizationError> {
    let mut flattened = Vec::new();

    for part in parts {
        let canonical = canonicalize_inner(defs, part, stack)?;
        match canonical {
            TypeExpr::Never => return Ok(TypeExpr::Never),
            TypeExpr::Intersection(inner) => flattened.extend(inner),
            other => flattened.push(other),
        }
    }

    let mut scalar_builtin: Option<BuiltinType> = None;
    let mut csv: Option<CsvConstraint> = None;
    let mut unit: Option<String> = None;
    let mut min_bound: Option<Literal> = None;
    let mut max_bound: Option<Literal> = None;
    let mut enum_values: Option<BTreeSet<Literal>> = None;
    let mut nullable = false;
    let mut others = BTreeSet::new();

    for part in flattened {
        match part {
            TypeExpr::Builtin(builtin) if is_scalar_builtin(builtin) => {
                if let Some(existing) = scalar_builtin {
                    if existing != builtin {
                        return Ok(TypeExpr::Never);
                    }
                } else {
                    scalar_builtin = Some(builtin);
                }
            }
            TypeExpr::Constructor(constructor) => match constructor.name {
                BuiltinConstructor::Csv => {
                    let next = CsvConstraint::from_constructor(&constructor);
                    csv = Some(match csv.take() {
                        Some(current) => match current.merge(next) {
                            Some(merged) => merged,
                            None => return Ok(TypeExpr::Never),
                        },
                        None => next,
                    });
                }
                BuiltinConstructor::Unit => {
                    let next = string_arg(&constructor, "value");
                    match &unit {
                        Some(current) if current != next => return Ok(TypeExpr::Never),
                        Some(_) => {}
                        None => unit = Some(next.to_string()),
                    }
                }
                BuiltinConstructor::Min => {
                    let next = numeric_arg_literal(&constructor, "value").clone();
                    min_bound = Some(match min_bound.take() {
                        Some(current) => stronger_min(&current, &next),
                        None => next,
                    });
                }
                BuiltinConstructor::Max => {
                    let next = numeric_arg_literal(&constructor, "value").clone();
                    max_bound = Some(match max_bound.take() {
                        Some(current) => stronger_max(&current, &next),
                        None => next,
                    });
                }
                BuiltinConstructor::Enum => {
                    let next = list_arg(&constructor, "values")
                        .iter()
                        .cloned()
                        .collect::<BTreeSet<_>>();
                    enum_values = Some(match enum_values.take() {
                        Some(current) => current.intersection(&next).cloned().collect(),
                        None => next,
                    });

                    if enum_values.as_ref().is_some_and(BTreeSet::is_empty) {
                        return Ok(TypeExpr::Never);
                    }
                }
                BuiltinConstructor::Nullable => {
                    nullable = true;
                }
            },
            other => {
                others.insert(other);
            }
        }
    }

    if let (Some(min), Some(max)) = (&min_bound, &max_bound) {
        if numeric_value(min) > numeric_value(max) {
            return Ok(TypeExpr::Never);
        }
    }

    let mut normalized = BTreeSet::new();

    if let Some(builtin) = scalar_builtin {
        normalized.insert(TypeExpr::Builtin(builtin));
    }
    if let Some(csv) = csv {
        normalized.insert(csv.into_type_expr());
    }
    if let Some(unit) = unit {
        normalized.insert(TypeExpr::Constructor(ConstructorExpr {
            name: BuiltinConstructor::Unit,
            args: BTreeMap::from([("value".to_string(), Literal::String(unit))]),
        }));
    }
    if let Some(min_bound) = min_bound {
        normalized.insert(TypeExpr::Constructor(ConstructorExpr {
            name: BuiltinConstructor::Min,
            args: BTreeMap::from([("value".to_string(), min_bound)]),
        }));
    }
    if let Some(max_bound) = max_bound {
        normalized.insert(TypeExpr::Constructor(ConstructorExpr {
            name: BuiltinConstructor::Max,
            args: BTreeMap::from([("value".to_string(), max_bound)]),
        }));
    }
    if let Some(enum_values) = enum_values {
        normalized.insert(TypeExpr::Constructor(ConstructorExpr {
            name: BuiltinConstructor::Enum,
            args: BTreeMap::from([(
                "values".to_string(),
                Literal::List(enum_values.into_iter().collect()),
            )]),
        }));
    }
    if nullable {
        normalized.insert(TypeExpr::Constructor(ConstructorExpr {
            name: BuiltinConstructor::Nullable,
            args: BTreeMap::new(),
        }));
    }
    normalized.extend(others);

    let mut parts = normalized.into_iter().collect::<Vec<_>>();
    if parts.len() == 1 {
        return Ok(parts.pop().expect("checked len"));
    }

    Ok(TypeExpr::Intersection(parts))
}

fn fingerprint_expr(expr: &TypeExpr) -> String {
    let mut out = String::new();
    write_expr_key(&mut out, expr);
    out
}

fn write_expr_key(out: &mut String, expr: &TypeExpr) {
    match expr {
        TypeExpr::Builtin(builtin) => {
            let _ = write!(out, "builtin({})", builtin.as_str());
        }
        TypeExpr::Ref(type_ref) => {
            let _ = write!(out, "ref({}", quote_string(&type_ref.name));
            if let Some(version) = &type_ref.version {
                let _ = write!(out, "@{}", quote_string(version));
            }
            out.push(')');
        }
        TypeExpr::Intersection(parts) => {
            out.push_str("intersection(");
            for (index, part) in parts.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_expr_key(out, part);
            }
            out.push(')');
        }
        TypeExpr::Constructor(constructor) => {
            let _ = write!(out, "ctor({}", constructor.name.as_str());
            for (name, value) in &constructor.args {
                let _ = write!(out, ";{}=", quote_string(name));
                write_literal_key(out, value);
            }
            out.push(')');
        }
        TypeExpr::Record(record) => {
            let _ = write!(out, "record(open={})[", record.open);
            for (index, field) in record.fields.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                let separator = if field.optional { "?:" } else { ":" };
                let _ = write!(out, "{}{}", quote_string(&field.name), separator);
                write_expr_key(out, &field.ty);
            }
            out.push(']');
        }
        TypeExpr::Collection(item) => {
            out.push_str("collection(");
            write_expr_key(out, item);
            out.push(')');
        }
        TypeExpr::Table(row) => {
            out.push_str("table(");
            write_expr_key(out, row);
            out.push(')');
        }
        TypeExpr::Never => out.push_str("never"),
    }
}

fn write_literal_key(out: &mut String, literal: &Literal) {
    match literal {
        Literal::Bool(value) => {
            let _ = write!(out, "bool({value})");
        }
        Literal::Integer(value) => {
            let _ = write!(out, "int({value})");
        }
        Literal::Float(value) => {
            let _ = write!(out, "float({:?})", value.into_inner());
        }
        Literal::String(value) => {
            let _ = write!(out, "string({})", quote_string(value));
        }
        Literal::List(values) => {
            out.push_str("list[");
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_literal_key(out, value);
            }
            out.push(']');
        }
    }
}

fn quote_string(value: &str) -> String {
    serde_json::to_string(value).expect("serializing a string literal should not fail")
}

fn is_scalar_builtin(builtin: BuiltinType) -> bool {
    matches!(
        builtin,
        BuiltinType::String
            | BuiltinType::Bool
            | BuiltinType::Int64
            | BuiltinType::Float64
            | BuiltinType::Date
            | BuiltinType::DateTime
    )
}

fn string_arg<'a>(constructor: &'a ConstructorExpr, name: &str) -> &'a str {
    match constructor.args.get(name) {
        Some(Literal::String(value)) => value.as_str(),
        _ => panic!("constructor arguments should be validated before canonicalization"),
    }
}

fn numeric_arg_literal<'a>(constructor: &'a ConstructorExpr, name: &str) -> &'a Literal {
    match constructor.args.get(name) {
        Some(Literal::Integer(_)) | Some(Literal::Float(_)) => constructor
            .args
            .get(name)
            .expect("just matched the numeric argument"),
        _ => panic!("constructor arguments should be validated before canonicalization"),
    }
}

fn list_arg<'a>(constructor: &'a ConstructorExpr, name: &str) -> &'a [Literal] {
    match constructor.args.get(name) {
        Some(Literal::List(values)) => values.as_slice(),
        _ => panic!("constructor arguments should be validated before canonicalization"),
    }
}

fn numeric_value(literal: &Literal) -> f64 {
    match literal {
        Literal::Integer(value) => *value as f64,
        Literal::Float(value) => value.into_inner(),
        _ => panic!("numeric_value called on non-numeric literal"),
    }
}

fn stronger_min(current: &Literal, next: &Literal) -> Literal {
    let current_value = numeric_value(current);
    let next_value = numeric_value(next);

    if next_value > current_value {
        return next.clone();
    }
    if next_value < current_value {
        return current.clone();
    }

    current.max(next).clone()
}

fn stronger_max(current: &Literal, next: &Literal) -> Literal {
    let current_value = numeric_value(current);
    let next_value = numeric_value(next);

    if next_value < current_value {
        return next.clone();
    }
    if next_value > current_value {
        return current.clone();
    }

    current.min(next).clone()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CsvConstraint {
    delimiter: Option<String>,
    header: Option<bool>,
}

impl CsvConstraint {
    fn from_constructor(constructor: &ConstructorExpr) -> Self {
        let delimiter = constructor.args.get("delimiter").map(|value| match value {
            Literal::String(value) => value.clone(),
            _ => panic!("csv delimiter should be validated as string"),
        });
        let header = constructor.args.get("header").map(|value| match value {
            Literal::Bool(value) => *value,
            _ => panic!("csv header should be validated as bool"),
        });

        Self { delimiter, header }
    }

    fn merge(self, next: Self) -> Option<Self> {
        let delimiter = match (self.delimiter, next.delimiter) {
            (Some(left), Some(right)) if left != right => return None,
            (Some(left), Some(_)) => Some(left),
            (Some(left), None) => Some(left),
            (None, Some(right)) => Some(right),
            (None, None) => None,
        };

        let header = match (self.header, next.header) {
            (Some(left), Some(right)) if left != right => return None,
            (Some(left), Some(_)) => Some(left),
            (Some(left), None) => Some(left),
            (None, Some(right)) => Some(right),
            (None, None) => None,
        };

        Some(Self { delimiter, header })
    }

    fn into_type_expr(self) -> TypeExpr {
        let mut args = BTreeMap::new();
        if let Some(delimiter) = self.delimiter {
            args.insert("delimiter".to_string(), Literal::String(delimiter));
        }
        if let Some(header) = self.header {
            args.insert("header".to_string(), Literal::Bool(header));
        }

        TypeExpr::Constructor(ConstructorExpr {
            name: BuiltinConstructor::Csv,
            args,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_type_id_round_trips() {
        let id = CanonicalTypeId::new("type_abc123");
        assert_eq!(id.as_str(), "type_abc123");
    }

    #[test]
    fn canonicalization_expands_local_aliases() {
        let mut defs = TypeDefinitions::default();
        defs.insert(crate::syntax::TypeDefinition::new(
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
        .expect("insert alias");

        let canonical = canonicalize(&defs, &TypeExpr::named_ref("WaterPotential"))
            .expect("canonicalization should succeed");

        assert_eq!(
            canonical,
            TypeExpr::intersection(vec![
                TypeExpr::Builtin(BuiltinType::Float64),
                TypeExpr::Constructor(ConstructorExpr {
                    name: BuiltinConstructor::Unit,
                    args: BTreeMap::from([(
                        "value".to_string(),
                        Literal::String("MPa".to_string()),
                    )]),
                }),
            ])
            .expect("non-empty intersection")
        );
    }

    #[test]
    fn conflicting_scalar_bases_reduce_to_never() {
        let defs = TypeDefinitions::default();

        let canonical = canonicalize(
            &defs,
            &TypeExpr::intersection(vec![TypeExpr::ref_("float64"), TypeExpr::ref_("string")])
                .expect("non-empty intersection"),
        )
        .expect("canonicalization should succeed");

        assert_eq!(canonical, TypeExpr::Never);
    }

    #[test]
    fn csv_constraints_merge_during_canonicalization() {
        let defs = TypeDefinitions::default();

        let canonical = canonicalize(
            &defs,
            &TypeExpr::intersection(vec![
                TypeExpr::Constructor(ConstructorExpr {
                    name: BuiltinConstructor::Csv,
                    args: BTreeMap::from([(
                        "delimiter".to_string(),
                        Literal::String(",".to_string()),
                    )]),
                }),
                TypeExpr::Constructor(ConstructorExpr {
                    name: BuiltinConstructor::Csv,
                    args: BTreeMap::from([("header".to_string(), Literal::Bool(true))]),
                }),
            ])
            .expect("non-empty intersection"),
        )
        .expect("canonicalization should succeed");

        assert_eq!(
            canonical,
            TypeExpr::Constructor(ConstructorExpr {
                name: BuiltinConstructor::Csv,
                args: BTreeMap::from([
                    ("delimiter".to_string(), Literal::String(",".to_string())),
                    ("header".to_string(), Literal::Bool(true)),
                ]),
            })
        );
    }

    #[test]
    fn min_max_conflicts_reduce_to_never() {
        let defs = TypeDefinitions::default();

        let canonical = canonicalize(
            &defs,
            &TypeExpr::intersection(vec![
                TypeExpr::Constructor(ConstructorExpr {
                    name: BuiltinConstructor::Min,
                    args: BTreeMap::from([("value".to_string(), Literal::Integer(10))]),
                }),
                TypeExpr::Constructor(ConstructorExpr {
                    name: BuiltinConstructor::Max,
                    args: BTreeMap::from([("value".to_string(), Literal::Integer(2))]),
                }),
            ])
            .expect("non-empty intersection"),
        )
        .expect("canonicalization should succeed");

        assert_eq!(canonical, TypeExpr::Never);
    }

    #[test]
    fn table_canonicalizes_through_named_row_aliases() {
        let mut defs = TypeDefinitions::default();
        defs.insert(crate::syntax::TypeDefinition::new(
            "Row",
            TypeExpr::Record(RecordExpr {
                fields: vec![RecordField {
                    name: "site_id".to_string(),
                    ty: TypeExpr::ref_("string"),
                    optional: false,
                }],
                open: false,
            }),
        ))
        .expect("insert row alias");

        let canonical = canonicalize(
            &defs,
            &TypeExpr::Table(Box::new(TypeExpr::named_ref("Row"))),
        )
        .expect("canonicalization should succeed");

        assert_eq!(
            canonical,
            TypeExpr::Table(Box::new(TypeExpr::Record(RecordExpr {
                fields: vec![RecordField {
                    name: "site_id".to_string(),
                    ty: TypeExpr::Builtin(BuiltinType::String),
                    optional: false,
                }],
                open: false,
            })))
        );
    }

    #[test]
    fn interner_reuses_existing_canonical_nodes() {
        let defs = TypeDefinitions::default();
        let mut interner = CanonicalTypeInterner::default();

        let left = interner
            .intern(
                &defs,
                &TypeExpr::intersection(vec![TypeExpr::ref_("float64")])
                    .expect("non-empty intersection"),
            )
            .expect("intern left");
        let right = interner
            .intern(&defs, &TypeExpr::ref_("float64"))
            .expect("intern right");

        assert_eq!(left.id, right.id);
        assert_eq!(interner.len(), 1);
    }
}
