//! Relation queries, evaluation, and conservative refinement logic.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::canonical::{CanonicalizationError, canonicalize};
use crate::syntax::{
    BuiltinConstructor, ConstructorExpr, Literal, RecordExpr, TypeDefinitions, TypeExpr,
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TypeRelation {
    Refines,
    Equivalent,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RelationVerdict {
    Holds,
    DoesNotHold,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelationQuery {
    pub left: TypeExpr,
    pub relation: TypeRelation,
    pub right: TypeExpr,
}

#[derive(Debug, Error)]
pub enum RelationError {
    #[error(transparent)]
    Canonicalization(#[from] CanonicalizationError),
}

pub fn equivalent(
    defs: &TypeDefinitions,
    left: &TypeExpr,
    right: &TypeExpr,
) -> Result<bool, RelationError> {
    Ok(canonicalize(defs, left)? == canonicalize(defs, right)?)
}

pub fn refines(
    defs: &TypeDefinitions,
    left: &TypeExpr,
    right: &TypeExpr,
) -> Result<bool, RelationError> {
    let left = canonicalize(defs, left)?;
    let right = canonicalize(defs, right)?;
    Ok(refines_canonical(&left, &right))
}

pub fn evaluate(
    defs: &TypeDefinitions,
    query: &RelationQuery,
) -> Result<RelationVerdict, RelationError> {
    let holds = match query.relation {
        TypeRelation::Equivalent => equivalent(defs, &query.left, &query.right)?,
        TypeRelation::Refines => refines(defs, &query.left, &query.right)?,
    };

    Ok(if holds {
        RelationVerdict::Holds
    } else {
        RelationVerdict::DoesNotHold
    })
}

fn refines_canonical(left: &TypeExpr, right: &TypeExpr) -> bool {
    if left == right {
        return true;
    }

    match (left, right) {
        (TypeExpr::Never, _) => true,
        (_, TypeExpr::Never) => false,
        (_, TypeExpr::Intersection(parts)) => {
            parts.iter().all(|part| refines_canonical(left, part))
        }
        (TypeExpr::Intersection(parts), _) => {
            parts.iter().any(|part| refines_canonical(part, right))
        }
        (TypeExpr::Collection(left_item), TypeExpr::Collection(right_item)) => {
            refines_canonical(left_item, right_item)
        }
        (TypeExpr::Table(left_row), TypeExpr::Table(right_row)) => {
            refines_canonical(left_row, right_row)
        }
        (TypeExpr::Table(left_row), TypeExpr::Collection(right_row)) => {
            refines_canonical(left_row, right_row)
        }
        (TypeExpr::Record(left_record), TypeExpr::Record(right_record)) => {
            record_refines(left_record, right_record)
        }
        (TypeExpr::Constructor(left_constructor), TypeExpr::Constructor(right_constructor)) => {
            constructor_refines(left_constructor, right_constructor)
        }
        _ => false,
    }
}

fn record_refines(left: &RecordExpr, right: &RecordExpr) -> bool {
    let left_fields = left
        .fields
        .iter()
        .map(|field| (field.name.as_str(), field))
        .collect::<BTreeMap<_, _>>();
    let right_fields = right
        .fields
        .iter()
        .map(|field| (field.name.as_str(), field))
        .collect::<BTreeMap<_, _>>();

    for target in &right.fields {
        match left_fields.get(target.name.as_str()) {
            Some(source) => {
                if source.optional && !target.optional {
                    return false;
                }
                if !refines_canonical(&source.ty, &target.ty) {
                    return false;
                }
            }
            None if target.optional => {}
            None => return false,
        }
    }

    if !right.open {
        for field_name in left_fields.keys() {
            if !right_fields.contains_key(field_name) {
                return false;
            }
        }
    }

    true
}

fn constructor_refines(left: &ConstructorExpr, right: &ConstructorExpr) -> bool {
    if left.name != right.name {
        return false;
    }

    match left.name {
        BuiltinConstructor::Csv => csv_refines(left, right),
        BuiltinConstructor::Unit => string_arg(left, "value") == string_arg(right, "value"),
        BuiltinConstructor::Min => {
            numeric_value(numeric_arg_literal(left, "value"))
                >= numeric_value(numeric_arg_literal(right, "value"))
        }
        BuiltinConstructor::Max => {
            numeric_value(numeric_arg_literal(left, "value"))
                <= numeric_value(numeric_arg_literal(right, "value"))
        }
        BuiltinConstructor::Enum => {
            let left_values = list_arg(left, "values");
            let right_values = list_arg(right, "values");
            left_values.iter().all(|value| right_values.contains(value))
        }
        BuiltinConstructor::Nullable => true,
    }
}

fn csv_refines(left: &ConstructorExpr, right: &ConstructorExpr) -> bool {
    for (name, expected) in &right.args {
        match left.args.get(name) {
            Some(actual) if actual == expected => {}
            _ => return false,
        }
    }

    true
}

fn string_arg<'a>(constructor: &'a ConstructorExpr, name: &str) -> &'a str {
    match constructor.args.get(name) {
        Some(Literal::String(value)) => value.as_str(),
        _ => panic!("constructor arguments should be validated before relation checks"),
    }
}

fn numeric_arg_literal<'a>(constructor: &'a ConstructorExpr, name: &str) -> &'a Literal {
    match constructor.args.get(name) {
        Some(Literal::Integer(_)) | Some(Literal::Float(_)) => constructor
            .args
            .get(name)
            .expect("just matched the numeric argument"),
        _ => panic!("constructor arguments should be validated before relation checks"),
    }
}

fn list_arg<'a>(constructor: &'a ConstructorExpr, name: &str) -> &'a [Literal] {
    match constructor.args.get(name) {
        Some(Literal::List(values)) => values.as_slice(),
        _ => panic!("constructor arguments should be validated before relation checks"),
    }
}

fn numeric_value(literal: &Literal) -> f64 {
    match literal {
        Literal::Integer(value) => *value as f64,
        Literal::Float(value) => value.into_inner(),
        _ => panic!("numeric_value called on non-numeric literal"),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::syntax::{BuiltinType, RecordField, TypeDefinition};

    #[test]
    fn relation_query_can_describe_refinement() {
        let query = RelationQuery {
            left: TypeExpr::intersection(vec![
                TypeExpr::ref_("float64"),
                TypeExpr::ref_("WaterPotential"),
            ])
            .expect("non-empty intersection"),
            relation: TypeRelation::Refines,
            right: TypeExpr::ref_("float64"),
        };

        assert_eq!(query.relation, TypeRelation::Refines);
    }

    #[test]
    fn equivalent_is_strict_after_alias_expansion() {
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
        .expect("insert alias");

        let expanded = TypeExpr::intersection(vec![
            TypeExpr::Builtin(BuiltinType::Float64),
            TypeExpr::Constructor(ConstructorExpr {
                name: BuiltinConstructor::Unit,
                args: BTreeMap::from([("value".to_string(), Literal::String("MPa".to_string()))]),
            }),
        ])
        .expect("non-empty intersection");

        assert!(
            equivalent(&defs, &TypeExpr::named_ref("WaterPotential"), &expanded)
                .expect("equivalence should succeed")
        );
    }

    #[test]
    fn table_refines_collection_of_the_same_row_type() {
        let defs = TypeDefinitions::default();
        let row = TypeExpr::Record(RecordExpr {
            fields: vec![RecordField {
                name: "site_id".to_string(),
                ty: TypeExpr::ref_("string"),
                optional: false,
            }],
            open: false,
        });

        let holds = refines(
            &defs,
            &TypeExpr::Table(Box::new(row.clone())),
            &TypeExpr::Collection(Box::new(row)),
        )
        .expect("refinement should succeed");

        assert!(holds);
    }

    #[test]
    fn open_record_width_subtyping_is_allowed() {
        let defs = TypeDefinitions::default();
        let source = TypeExpr::Record(RecordExpr {
            fields: vec![
                RecordField {
                    name: "a".to_string(),
                    ty: TypeExpr::ref_("int64"),
                    optional: false,
                },
                RecordField {
                    name: "b".to_string(),
                    ty: TypeExpr::ref_("string"),
                    optional: false,
                },
            ],
            open: false,
        });
        let target = TypeExpr::Record(RecordExpr {
            fields: vec![RecordField {
                name: "a".to_string(),
                ty: TypeExpr::ref_("int64"),
                optional: false,
            }],
            open: true,
        });

        assert!(refines(&defs, &source, &target).expect("refinement should succeed"));
    }

    #[test]
    fn closed_record_width_subtyping_is_rejected() {
        let defs = TypeDefinitions::default();
        let source = TypeExpr::Record(RecordExpr {
            fields: vec![
                RecordField {
                    name: "a".to_string(),
                    ty: TypeExpr::ref_("int64"),
                    optional: false,
                },
                RecordField {
                    name: "b".to_string(),
                    ty: TypeExpr::ref_("string"),
                    optional: false,
                },
            ],
            open: false,
        });
        let target = TypeExpr::Record(RecordExpr {
            fields: vec![RecordField {
                name: "a".to_string(),
                ty: TypeExpr::ref_("int64"),
                optional: false,
            }],
            open: false,
        });

        assert!(!refines(&defs, &source, &target).expect("refinement should succeed"));
    }

    #[test]
    fn stronger_min_refines_weaker_min() {
        let defs = TypeDefinitions::default();
        let stronger = TypeExpr::Constructor(ConstructorExpr {
            name: BuiltinConstructor::Min,
            args: BTreeMap::from([("value".to_string(), Literal::Integer(10))]),
        });
        let weaker = TypeExpr::Constructor(ConstructorExpr {
            name: BuiltinConstructor::Min,
            args: BTreeMap::from([("value".to_string(), Literal::Integer(2))]),
        });

        assert!(refines(&defs, &stronger, &weaker).expect("refinement should succeed"));
        assert!(!refines(&defs, &weaker, &stronger).expect("refinement should succeed"));
    }

    #[test]
    fn enum_subset_refines_superset() {
        let defs = TypeDefinitions::default();
        let subset = TypeExpr::Constructor(ConstructorExpr {
            name: BuiltinConstructor::Enum,
            args: BTreeMap::from([(
                "values".to_string(),
                Literal::List(vec![Literal::String("oak".to_string())]),
            )]),
        });
        let superset = TypeExpr::Constructor(ConstructorExpr {
            name: BuiltinConstructor::Enum,
            args: BTreeMap::from([(
                "values".to_string(),
                Literal::List(vec![
                    Literal::String("oak".to_string()),
                    Literal::String("pine".to_string()),
                ]),
            )]),
        });

        assert!(refines(&defs, &subset, &superset).expect("refinement should succeed"));
        assert!(!refines(&defs, &superset, &subset).expect("refinement should succeed"));
    }
}
