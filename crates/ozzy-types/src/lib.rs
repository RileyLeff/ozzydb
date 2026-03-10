#![recursion_limit = "4096"]

//! OzzyDB Types - v4 type system foundation.
//!
//! This crate holds the core data model for the OzzyDB v4 type system:
//! syntax trees, canonical forms, registry objects, typed ports,
//! conformance records, and verification reports/witnesses.

pub mod canonical;
pub mod conformance;
pub mod parse;
pub mod ports;
pub mod registry;
pub mod relations;
pub mod schema;
pub mod syntax;
pub mod verify;

pub use canonical::{CanonicalType, CanonicalTypeId};
pub use conformance::{
    ConformanceRecord, ConformanceStatus, VerificationAttempt, VerificationFailure,
};
pub use parse::{TypeParseError, parse_type_expr, parse_type_ref};
pub use ports::{TypedPort, TypedPortSet};
pub use registry::{RegistryError, TypeRegistry, TypeVersion, TypeVersionId};
pub use relations::{RelationQuery, RelationVerdict, TypeRelation};
pub use schema::{
    FieldInfo, SchemaError, SchemaInfo, extract_parquet_schema, get_parquet_row_count,
};
pub use syntax::{
    BuiltinConstructor, BuiltinType, ConstructorExpr, Literal, RecordExpr, RecordField,
    TypeDefinition, TypeDefinitions, TypeExpr, TypeLanguageError, TypeRefExpr,
};
pub use verify::{
    BuiltinVerifierRegistry, CsvWitness, RecordFieldPlan, RecordWitness, TableColumnWitness,
    TableWitness, VerificationError, VerificationInput, VerificationPlan, VerificationReport,
    VerificationVerdict, WitnessError,
};

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn crate_surface_supports_basic_type_registration() {
        let mut registry = TypeRegistry::default();
        let type_version = TypeVersion::new(
            "std/WaterPotential",
            "1",
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
        );
        let type_id = type_version.id.clone();

        registry
            .insert(type_version)
            .expect("insert should succeed");

        let stored = registry.get(&type_id).expect("type should exist");
        assert_eq!(stored.name, "std/WaterPotential");
        assert_eq!(stored.version, "1");
    }
}
