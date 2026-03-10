//! Registry-facing type version objects.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::canonical::CanonicalTypeId;
use crate::syntax::{TypeExpr, TypeRefExpr};

/// Public identifier for a published type version.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TypeVersionId(String);

impl TypeVersionId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A published type version in the v4 registry model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TypeVersion {
    pub id: TypeVersionId,
    pub name: String,
    pub version: String,
    pub expr: TypeExpr,
    pub canonical: Option<CanonicalTypeId>,
}

impl TypeVersion {
    pub fn new(name: impl Into<String>, version: impl Into<String>, expr: TypeExpr) -> Self {
        let name = name.into();
        let version = version.into();
        let id = TypeVersionId::new(format!("{name}@{version}"));

        Self {
            id,
            name,
            version,
            expr,
            canonical: None,
        }
    }
}

/// Registry errors should be explicit and non-lossy.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RegistryError {
    #[error("type version '{name}@{version}' already exists")]
    DuplicateTypeVersion { name: String, version: String },
    #[error("published type reference '{name}@{version}' could not be resolved")]
    UnknownTypeVersionReference { name: String, version: String },
}

/// Minimal in-memory registry surface for Phase 1.1 scaffolding.
#[derive(Debug, Default, Clone)]
pub struct TypeRegistry {
    types: BTreeMap<TypeVersionId, TypeVersion>,
}

impl TypeRegistry {
    pub fn insert(&mut self, type_version: TypeVersion) -> Result<(), RegistryError> {
        let duplicate_name_and_version = self.types.values().any(|existing| {
            existing.name == type_version.name && existing.version == type_version.version
        });

        if self.types.contains_key(&type_version.id) || duplicate_name_and_version {
            return Err(RegistryError::DuplicateTypeVersion {
                name: type_version.name.clone(),
                version: type_version.version.clone(),
            });
        }

        self.types.insert(type_version.id.clone(), type_version);
        Ok(())
    }

    pub fn get(&self, id: &TypeVersionId) -> Option<&TypeVersion> {
        self.types.get(id)
    }

    pub fn get_by_name_version(&self, name: &str, version: &str) -> Option<&TypeVersion> {
        self.types
            .values()
            .find(|type_version| type_version.name == name && type_version.version == version)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&TypeVersionId, &TypeVersion)> {
        self.types.iter()
    }

    pub fn len(&self) -> usize {
        self.types.len()
    }

    pub fn resolve_ref(&self, type_ref: &TypeRefExpr) -> Result<&TypeVersion, RegistryError> {
        let version = type_ref.version.as_deref().ok_or_else(|| {
            RegistryError::UnknownTypeVersionReference {
                name: type_ref.name.clone(),
                version: "<unversioned>".to_string(),
            }
        })?;

        self.get_by_name_version(&type_ref.name, version)
            .ok_or_else(|| RegistryError::UnknownTypeVersionReference {
                name: type_ref.name.clone(),
                version: version.to_string(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_type_versions_are_rejected_explicitly() {
        let mut registry = TypeRegistry::default();
        let type_version = TypeVersion::new("std/WaterPotential", "1", TypeExpr::ref_("float64"));

        registry
            .insert(type_version.clone())
            .expect("first insert should succeed");

        let err = registry
            .insert(type_version)
            .expect_err("second insert should fail");
        assert_eq!(
            err,
            RegistryError::DuplicateTypeVersion {
                name: "std/WaterPotential".to_string(),
                version: "1".to_string(),
            }
        );
    }

    #[test]
    fn duplicate_name_and_version_are_rejected_even_if_id_differs() {
        let mut registry = TypeRegistry::default();
        let canonical = TypeVersion::new("std/WaterPotential", "1", TypeExpr::ref_("float64"));
        let mismatched_id = TypeVersion {
            id: TypeVersionId::new("custom-id"),
            ..canonical.clone()
        };

        registry
            .insert(canonical)
            .expect("first insert should succeed");

        let err = registry
            .insert(mismatched_id)
            .expect_err("duplicate name/version should fail");
        assert_eq!(
            err,
            RegistryError::DuplicateTypeVersion {
                name: "std/WaterPotential".to_string(),
                version: "1".to_string(),
            }
        );
    }

    #[test]
    fn published_type_refs_can_be_resolved_by_name_and_version() {
        let mut registry = TypeRegistry::default();
        let type_version = TypeVersion::new("std/WaterPotential", "1", TypeExpr::ref_("float64"));
        registry
            .insert(type_version)
            .expect("insert should succeed");

        let resolved = registry
            .resolve_ref(&TypeRefExpr::new(
                "std/WaterPotential",
                Some("1".to_string()),
            ))
            .expect("published ref should resolve");

        assert_eq!(resolved.name, "std/WaterPotential");
        assert_eq!(resolved.version, "1");
    }
}
