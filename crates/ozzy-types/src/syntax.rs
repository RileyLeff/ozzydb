//! Surface syntax AST for the v4 type language.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A reference to a named type, optionally pinned to a published version.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Literal {
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(String),
}

/// A constructor application like `csv(delimiter=",", header=true)`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConstructorExpr {
    pub name: String,
    #[serde(default)]
    pub args: BTreeMap<String, Literal>,
}

/// A single record field in a record type expression.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecordField {
    pub name: String,
    pub ty: TypeExpr,
    #[serde(default)]
    pub optional: bool,
}

/// A record expression. Records are closed by default; `open=true` models `...`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecordExpr {
    #[serde(default)]
    pub fields: Vec<RecordField>,
    #[serde(default)]
    pub open: bool,
}

/// The v1 surface syntax tree for OzzyDB type expressions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TypeExpr {
    Ref(TypeRefExpr),
    Intersection(Vec<TypeExpr>),
    Constructor(ConstructorExpr),
    Record(RecordExpr),
    Collection(Box<TypeExpr>),
    Table(RecordExpr),
    Never,
}

impl TypeExpr {
    pub fn ref_(name: impl Into<String>) -> Self {
        Self::Ref(TypeRefExpr::new(name, None))
    }

    pub fn intersection(parts: Vec<TypeExpr>) -> Self {
        Self::Intersection(parts)
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
}
