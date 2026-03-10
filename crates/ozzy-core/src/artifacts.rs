use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ArtifactManifestEntry {
    pub artifact_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ArtifactManifest {
    Bundle {
        entries: BTreeMap<String, ArtifactManifestEntry>,
    },
    Collection {
        items: Vec<ArtifactManifestEntry>,
    },
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ArtifactManifestError {
    #[error("bundle manifests cannot contain empty entry names")]
    EmptyBundleEntryName,
}

impl ArtifactManifest {
    pub fn validate(&self) -> Result<(), ArtifactManifestError> {
        if let Self::Bundle { entries } = self {
            if entries.keys().any(|name| name.trim().is_empty()) {
                return Err(ArtifactManifestError::EmptyBundleEntryName);
            }
        }

        Ok(())
    }

    pub fn referenced_artifact_ids(&self) -> Vec<Uuid> {
        match self {
            Self::Bundle { entries } => entries.values().map(|entry| entry.artifact_id).collect(),
            Self::Collection { items } => items.iter().map(|entry| entry.artifact_id).collect(),
        }
    }

    pub fn referenced_artifact_id_set(&self) -> BTreeSet<Uuid> {
        self.referenced_artifact_ids().into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_manifest_rejects_empty_entry_names() {
        let manifest = ArtifactManifest::Bundle {
            entries: BTreeMap::from([(
                "".to_string(),
                ArtifactManifestEntry {
                    artifact_id: Uuid::new_v4(),
                },
            )]),
        };

        assert_eq!(
            manifest.validate(),
            Err(ArtifactManifestError::EmptyBundleEntryName)
        );
    }

    #[test]
    fn referenced_artifact_ids_preserve_collection_order() {
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let manifest = ArtifactManifest::Collection {
            items: vec![
                ArtifactManifestEntry { artifact_id: first },
                ArtifactManifestEntry {
                    artifact_id: second,
                },
                ArtifactManifestEntry { artifact_id: first },
            ],
        };

        assert_eq!(
            manifest.referenced_artifact_ids(),
            vec![first, second, first]
        );
        assert_eq!(manifest.referenced_artifact_id_set().len(), 2);
    }
}
