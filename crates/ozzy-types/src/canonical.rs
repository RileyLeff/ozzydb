//! Canonical type identifiers and normalized type containers.

use serde::{Deserialize, Serialize};

use crate::syntax::TypeExpr;

/// Stable identifier for a canonicalized type node.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CanonicalTypeId(String);

impl CanonicalTypeId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A canonical type node stored in the registry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CanonicalType {
    pub id: CanonicalTypeId,
    pub expr: TypeExpr,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_type_id_round_trips() {
        let id = CanonicalTypeId::new("type_abc123");
        assert_eq!(id.as_str(), "type_abc123");
    }
}
