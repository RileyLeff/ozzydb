//! Typed input and output port definitions.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::syntax::TypeRefExpr;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TypedPort {
    pub ty: TypeRefExpr,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TypedPortSet {
    pub ports: BTreeMap<String, TypedPort>,
}

impl TypedPort {
    pub fn new(ty: TypeRefExpr) -> Self {
        Self {
            ty,
            description: None,
        }
    }
}

impl TypedPortSet {
    pub fn insert(&mut self, name: impl Into<String>, port: TypedPort) -> Option<TypedPort> {
        self.ports.insert(name.into(), port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_port_set_holds_named_ports() {
        let mut ports = TypedPortSet::default();
        ports.insert(
            "raw".to_string(),
            TypedPort::new(TypeRefExpr::new(
                "std/RawWaterPotentialCsv",
                Some("1".to_string()),
            )),
        );

        assert!(ports.ports.contains_key("raw"));
    }
}
