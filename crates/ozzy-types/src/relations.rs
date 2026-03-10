//! Relation query types for refinement and equivalence.

use serde::{Deserialize, Serialize};

use crate::syntax::TypeExpr;

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RelationQuery {
    pub left: TypeExpr,
    pub relation: TypeRelation,
    pub right: TypeExpr,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relation_query_can_describe_refinement() {
        let query = RelationQuery {
            left: TypeExpr::intersection(vec![
                TypeExpr::ref_("float64"),
                TypeExpr::ref_("WaterPotential"),
            ]),
            relation: TypeRelation::Refines,
            right: TypeExpr::ref_("float64"),
        };

        assert_eq!(query.relation, TypeRelation::Refines);
    }
}
