//! Shared v4 inspection response builders.

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use chrono::{DateTime, Utc};
use ozzy_types::ports::TypedPortSet;
use ozzy_types::syntax::{TypeExpr, TypeRefExpr};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::db::models::Commit;
use crate::registry::{
    PublishedProjectRevision, RegistrySnapshot, RuntimeTransformDef, VersionedName,
};

#[derive(Debug, Serialize)]
pub(crate) struct EndpointSummary {
    pub name: String,
    pub description: Option<String>,
    pub inputs: Vec<TypedPortDetail>,
    pub params: Vec<ParamSummary>,
    pub node_count: usize,
    pub edge_count: usize,
    pub terminal_node: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ParamSummary {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub description: Option<String>,
    pub default: Option<Value>,
}

#[derive(Debug, Serialize)]
pub(crate) struct EndpointDetail {
    pub name: String,
    pub description: Option<String>,
    pub commit_sha: String,
    pub project_revision_id: Uuid,
    pub registry_revision_id: Uuid,
    pub terminal_node: String,
    pub inputs: Vec<TypedPortDetail>,
    pub params: Vec<ParamDetail>,
    pub nodes: Vec<NodeDetail>,
    pub edges: Vec<EdgeDetail>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ParamDetail {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub description: Option<String>,
    pub default: Option<Value>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    #[serde(rename = "enum")]
    pub enum_values: Option<Vec<Value>>,
    pub binds: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct NodeDetail {
    pub name: String,
    pub params: HashMap<String, Value>,
    pub transform: TransformInspection,
}

#[derive(Debug, Serialize)]
pub(crate) struct EdgeDetail {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct TypedPortDetail {
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "type")]
    pub ty: TypeRefDetail,
}

#[derive(Debug, Serialize)]
pub(crate) struct TypeRefDetail {
    pub reference: String,
    pub name: String,
    pub version: String,
    pub type_version_id: Uuid,
    pub canonical_type_key: String,
    pub expr: TypeExpr,
}

#[derive(Debug, Serialize)]
pub(crate) struct TransformInspection {
    pub authored_name: String,
    pub versioned_name: String,
    pub transform_version_id: Uuid,
    pub description: Option<String>,
    pub source: Option<String>,
    pub command: Option<String>,
    pub network: bool,
    pub secrets: Vec<String>,
    pub params_schema: Value,
    pub environment: TransformEnvironmentRef,
    pub inputs: Vec<TypedPortDetail>,
    pub outputs: Vec<TypedPortDetail>,
}

#[derive(Debug, Serialize)]
pub(crate) struct TransformEnvironmentRef {
    pub versioned_name: String,
    pub environment_version_id: Uuid,
}

#[derive(Debug, Serialize)]
pub(crate) struct ProjectRevisionDetail {
    pub id: Uuid,
    pub registry_revision_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub project_meta: Value,
    pub environments: Vec<PublishedEnvironmentInspection>,
    pub transforms: Vec<TransformInspection>,
    pub endpoints: Vec<ProjectRevisionEndpointSummary>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PublishedEnvironmentInspection {
    pub authored_name: String,
    pub versioned_name: String,
    pub environment_version_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub definition: ozzy_core::toml_spec::PublishedEnvironmentDef,
}

#[derive(Debug, Serialize)]
pub(crate) struct ProjectRevisionEndpointSummary {
    pub name: String,
    pub description: Option<String>,
    pub inputs: Vec<TypedPortDetail>,
    pub params: Vec<ParamSummary>,
    pub node_count: usize,
    pub edge_count: usize,
    pub terminal_node: String,
}

pub(crate) fn build_endpoint_summary(
    snapshot: &RegistrySnapshot,
    endpoint_name: &str,
    endpoint: &ozzy_core::toml_spec::EndpointDef,
) -> Result<EndpointSummary> {
    Ok(EndpointSummary {
        name: endpoint_name.to_string(),
        description: endpoint.description.clone(),
        inputs: inspect_typed_ports(snapshot, &endpoint.inputs)?,
        params: extract_param_summaries(endpoint),
        node_count: endpoint.nodes.len(),
        edge_count: endpoint.edges.len(),
        terminal_node: terminal_node_name(endpoint)?,
    })
}

pub(crate) fn build_endpoint_detail(
    commit: &Commit,
    published: &PublishedProjectRevision,
    endpoint_name: &str,
    endpoint: &ozzy_core::toml_spec::EndpointDef,
) -> Result<EndpointDetail> {
    let mut node_names = endpoint.nodes.keys().cloned().collect::<Vec<_>>();
    node_names.sort();

    let mut nodes = Vec::with_capacity(node_names.len());
    for node_name in node_names {
        let node = endpoint
            .nodes
            .get(&node_name)
            .expect("node names derived from endpoint map");
        let runtime_transform = published
            .runtime
            .transforms
            .get(&node.transform)
            .ok_or_else(|| anyhow::anyhow!("missing runtime transform '{}'", node.transform))?;
        nodes.push(NodeDetail {
            name: node_name,
            params: node.params.clone(),
            transform: inspect_runtime_transform(
                published.snapshot.as_ref(),
                &node.transform,
                runtime_transform,
            )?,
        });
    }

    Ok(EndpointDetail {
        name: endpoint_name.to_string(),
        description: endpoint.description.clone(),
        commit_sha: commit.git_commit_sha.clone(),
        project_revision_id: published.row.id,
        registry_revision_id: published.row.registry_revision_id,
        terminal_node: terminal_node_name(endpoint)?,
        inputs: inspect_typed_ports(published.snapshot.as_ref(), &endpoint.inputs)?,
        params: extract_param_details(endpoint),
        nodes,
        edges: extract_edges(endpoint),
    })
}

pub(crate) fn build_project_revision_detail(
    published: &PublishedProjectRevision,
) -> Result<ProjectRevisionDetail> {
    let mut environments = published
        .environment_bindings
        .iter()
        .map(|(authored_name, versioned_name)| {
            inspect_published_environment(
                published.snapshot.as_ref(),
                authored_name,
                versioned_name,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    environments.sort_by(|a, b| a.authored_name.cmp(&b.authored_name));

    let mut transform_names = published
        .runtime
        .transforms
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    transform_names.sort();
    let mut transforms = Vec::with_capacity(transform_names.len());
    for authored_name in transform_names {
        let runtime_transform = published
            .runtime
            .transforms
            .get(&authored_name)
            .expect("transform names derived from runtime map");
        transforms.push(inspect_runtime_transform(
            published.snapshot.as_ref(),
            &authored_name,
            runtime_transform,
        )?);
    }

    let mut endpoint_names = published.endpoints.keys().cloned().collect::<Vec<_>>();
    endpoint_names.sort();
    let mut endpoints = Vec::with_capacity(endpoint_names.len());
    for endpoint_name in endpoint_names {
        let endpoint = published
            .endpoints
            .get(&endpoint_name)
            .expect("endpoint names derived from endpoint map");
        endpoints.push(ProjectRevisionEndpointSummary {
            name: endpoint_name,
            description: endpoint.description.clone(),
            inputs: inspect_typed_ports(published.snapshot.as_ref(), &endpoint.inputs)?,
            params: extract_param_summaries(endpoint),
            node_count: endpoint.nodes.len(),
            edge_count: endpoint.edges.len(),
            terminal_node: terminal_node_name(endpoint)?,
        });
    }

    Ok(ProjectRevisionDetail {
        id: published.row.id,
        registry_revision_id: published.row.registry_revision_id,
        created_at: published.row.created_at,
        project_meta: published.project_meta.clone(),
        environments,
        transforms,
        endpoints,
    })
}

fn inspect_runtime_transform(
    snapshot: &RegistrySnapshot,
    authored_name: &str,
    runtime_transform: &RuntimeTransformDef,
) -> Result<TransformInspection> {
    Ok(TransformInspection {
        authored_name: authored_name.to_string(),
        versioned_name: runtime_transform.versioned_name.to_string(),
        transform_version_id: runtime_transform.row_id,
        description: runtime_transform.description.clone(),
        source: runtime_transform.source.clone(),
        command: runtime_transform.command.clone(),
        network: runtime_transform.network,
        secrets: runtime_transform.secrets.clone(),
        params_schema: runtime_transform.params_schema.clone(),
        environment: TransformEnvironmentRef {
            versioned_name: runtime_transform.environment.versioned_name.to_string(),
            environment_version_id: runtime_transform.environment.row_id,
        },
        inputs: inspect_typed_ports(snapshot, &runtime_transform.inputs)?,
        outputs: inspect_typed_ports(snapshot, &runtime_transform.outputs)?,
    })
}

fn inspect_published_environment(
    snapshot: &RegistrySnapshot,
    authored_name: &str,
    versioned_name: &VersionedName,
) -> Result<PublishedEnvironmentInspection> {
    let row = snapshot
        .environment(&versioned_name.name, &versioned_name.version)
        .ok_or_else(|| anyhow::anyhow!("missing published environment '{}'", versioned_name))?;
    let definition: ozzy_core::toml_spec::PublishedEnvironmentDef =
        serde_json::from_value(row.definition.clone())?;
    Ok(PublishedEnvironmentInspection {
        authored_name: authored_name.to_string(),
        versioned_name: versioned_name.to_string(),
        environment_version_id: row.id,
        created_at: row.created_at,
        definition,
    })
}

fn inspect_typed_ports(
    snapshot: &RegistrySnapshot,
    ports: &TypedPortSet,
) -> Result<Vec<TypedPortDetail>> {
    let mut names = ports.ports.keys().cloned().collect::<Vec<_>>();
    names.sort();

    let mut details = Vec::with_capacity(names.len());
    for name in names {
        let port = ports
            .ports
            .get(&name)
            .expect("port names derived from typed port set");
        details.push(TypedPortDetail {
            name,
            description: port.description.clone(),
            ty: inspect_type_ref(snapshot, &port.ty)?,
        });
    }

    Ok(details)
}

fn inspect_type_ref(snapshot: &RegistrySnapshot, type_ref: &TypeRefExpr) -> Result<TypeRefDetail> {
    let (type_version, stored_row) = snapshot.resolve_type_ref(type_ref)?;
    let canonical = snapshot
        .canonical_type_by_row_id(stored_row.canonical_type_id)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "type '{}'@'{}' references missing canonical type '{}'",
                stored_row.name,
                stored_row.version,
                stored_row.canonical_type_id
            )
        })?;

    Ok(TypeRefDetail {
        reference: type_ref_string(type_ref),
        name: type_version.name.clone(),
        version: type_version.version.clone(),
        type_version_id: stored_row.id,
        canonical_type_key: canonical.id.as_str().to_string(),
        expr: type_version.expr.clone(),
    })
}

fn terminal_node_name(endpoint: &ozzy_core::toml_spec::EndpointDef) -> Result<String> {
    let node_names: HashSet<&str> = endpoint.nodes.keys().map(String::as_str).collect();
    let mut has_outgoing: HashSet<&str> = HashSet::new();

    for edge in &endpoint.edges {
        if node_names.contains(edge.from.as_str()) {
            let to_node = edge.to.split('.').next().unwrap_or(&edge.to);
            if node_names.contains(to_node) && edge.from.as_str() != to_node {
                has_outgoing.insert(edge.from.as_str());
            }
        }
    }

    let mut terminals = node_names
        .into_iter()
        .filter(|name| !has_outgoing.contains(name))
        .collect::<Vec<_>>();
    terminals.sort();

    match terminals.as_slice() {
        [single] => Ok((*single).to_string()),
        [] => anyhow::bail!("endpoint has no terminal node"),
        many => anyhow::bail!("endpoint has multiple terminal nodes: {:?}", many),
    }
}

fn extract_param_summaries(def: &ozzy_core::toml_spec::EndpointDef) -> Vec<ParamSummary> {
    let mut names = def.params.keys().cloned().collect::<Vec<_>>();
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let param = def.params.get(&name).expect("param name derived from map");
            ParamSummary {
                name,
                type_: param.type_.clone(),
                description: param.description.clone(),
                default: param.default.clone(),
            }
        })
        .collect()
}

fn extract_param_details(def: &ozzy_core::toml_spec::EndpointDef) -> Vec<ParamDetail> {
    let mut names = def.params.keys().cloned().collect::<Vec<_>>();
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let param = def.params.get(&name).expect("param name derived from map");
            ParamDetail {
                name,
                type_: param.type_.clone(),
                description: param.description.clone(),
                default: param.default.clone(),
                min: param.min,
                max: param.max,
                enum_values: param.enum_values.clone(),
                binds: param.binds.clone(),
            }
        })
        .collect()
}

fn extract_edges(def: &ozzy_core::toml_spec::EndpointDef) -> Vec<EdgeDetail> {
    def.edges
        .iter()
        .map(|edge| EdgeDetail {
            from: edge.from.clone(),
            to: edge.to.clone(),
        })
        .collect()
}

fn type_ref_string(type_ref: &TypeRefExpr) -> String {
    match &type_ref.version {
        Some(version) => format!("{}@{}", type_ref.name, version),
        None => type_ref.name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ozzy_core::toml_spec::{EdgeDef, EndpointDef, NodeDef};

    #[test]
    fn terminal_node_name_returns_single_sink() {
        let endpoint = EndpointDef {
            description: None,
            inputs: TypedPortSet::default(),
            params: HashMap::new(),
            nodes: HashMap::from([
                (
                    "source".to_string(),
                    NodeDef {
                        transform: "source".to_string(),
                        params: HashMap::new(),
                    },
                ),
                (
                    "sink".to_string(),
                    NodeDef {
                        transform: "sink".to_string(),
                        params: HashMap::new(),
                    },
                ),
            ]),
            edges: vec![EdgeDef {
                from: "source".to_string(),
                to: "sink.input".to_string(),
            }],
        };

        assert_eq!(terminal_node_name(&endpoint).unwrap(), "sink");
    }

    #[test]
    fn terminal_node_name_rejects_multiple_sinks() {
        let endpoint = EndpointDef {
            description: None,
            inputs: TypedPortSet::default(),
            params: HashMap::new(),
            nodes: HashMap::from([
                (
                    "left".to_string(),
                    NodeDef {
                        transform: "left".to_string(),
                        params: HashMap::new(),
                    },
                ),
                (
                    "right".to_string(),
                    NodeDef {
                        transform: "right".to_string(),
                        params: HashMap::new(),
                    },
                ),
            ]),
            edges: vec![],
        };

        let err = terminal_node_name(&endpoint).expect_err("multiple terminals must error");
        assert!(err.to_string().contains("multiple terminal nodes"));
    }

    #[test]
    fn type_ref_string_formats_versioned_refs() {
        assert_eq!(
            type_ref_string(&TypeRefExpr::new("std/Foo", Some("2".to_string()))),
            "std/Foo@2"
        );
        assert_eq!(
            type_ref_string(&TypeRefExpr::new("LocalType", None)),
            "LocalType"
        );
    }
}
