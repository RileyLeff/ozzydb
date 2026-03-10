//! v4 `ozzy.toml` parser and validator.
//!
//! `ozzy.toml` remains the authored declaration layer, but its job in v4 is to
//! describe typed environments, transforms, and endpoints that later compile
//! into first-class registry objects. This module is intentionally strict: it
//! parses raw TOML into typed Rust data and rejects malformed or ambiguous
//! authored state instead of silently defaulting it.

use std::collections::{HashMap, VecDeque};

use ozzy_types::parse::{TypeParseError, parse_type_expr, parse_type_ref};
use ozzy_types::ports::{TypedPort, TypedPortSet};
use ozzy_types::syntax::{BuiltinType, TypeDefinition, TypeDefinitions};
use serde::{Deserialize, Serialize};
use thiserror::Error;

fn is_valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

#[derive(Debug, Clone, Serialize)]
pub struct OzzyToml {
    pub project: ProjectSection,
    pub git: Option<GitSection>,
    pub remote: Option<RemoteSection>,
    pub types: TypeDefinitions,
    pub environments: HashMap<String, EnvironmentDef>,
    pub transforms: HashMap<String, TransformDef>,
    pub endpoints: HashMap<String, EndpointDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSection {
    pub name: String,
    pub owner: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitSection {
    pub provider: String,
    pub repo: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteSection {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentDef {
    #[serde(default)]
    pub base: Option<String>,
    #[serde(default)]
    pub lockfile: Option<String>,
    #[serde(default)]
    pub dockerfile: Option<String>,
    #[serde(default)]
    pub image: Option<String>,
}

impl EnvironmentDef {
    pub fn tier(&self) -> Option<EnvironmentTier> {
        match (
            self.base.as_ref(),
            self.lockfile.as_ref(),
            self.dockerfile.as_ref(),
            self.image.as_ref(),
        ) {
            (Some(base), Some(lockfile), None, None) => Some(EnvironmentTier::BaseLockfile {
                base: base.clone(),
                lockfile: lockfile.clone(),
            }),
            (None, None, Some(dockerfile), None) => Some(EnvironmentTier::Dockerfile {
                dockerfile: dockerfile.clone(),
            }),
            (None, None, None, Some(image)) => Some(EnvironmentTier::Prebuilt {
                image: image.clone(),
            }),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvironmentTier {
    BaseLockfile { base: String, lockfile: String },
    Dockerfile { dockerfile: String },
    Prebuilt { image: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PublishedEnvironmentDef {
    BaseLockfile {
        base: String,
        installer: BaseLockfileInstaller,
        lockfile_content: String,
    },
    Dockerfile {
        dockerfile_content: String,
    },
    Prebuilt {
        image: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BaseLockfileInstaller {
    PipRequirements,
}

impl BaseLockfileInstaller {
    pub fn as_identity_str(&self) -> &'static str {
        match self {
            Self::PipRequirements => "pip_requirements",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformDef {
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    pub environment: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub inputs: TypedPortSet,
    #[serde(default)]
    pub outputs: TypedPortSet,
    #[serde(default)]
    pub params: HashMap<String, ParamDef>,
    #[serde(default)]
    pub network: bool,
    #[serde(default)]
    pub secrets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamDef {
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub default: Option<serde_json::Value>,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default, rename = "enum")]
    pub enum_values: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointDef {
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub params: HashMap<String, EndpointParamDef>,
    #[serde(default)]
    pub nodes: HashMap<String, NodeDef>,
    #[serde(default)]
    pub edges: Vec<EdgeDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointParamDef {
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(default)]
    pub default: Option<serde_json::Value>,
    pub binds: String,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default, rename = "enum")]
    pub enum_values: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeDef {
    pub transform: String,
    #[serde(default)]
    pub params: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeDef {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EdgeSource {
    Data(String),
    Collection(String),
    Endpoint(String),
    Node(String),
}

pub fn parse_edge_source(from: &str) -> EdgeSource {
    if let Some(rest) = from.strip_prefix("data:") {
        EdgeSource::Data(rest.to_string())
    } else if let Some(rest) = from.strip_prefix("collection:") {
        EdgeSource::Collection(rest.to_string())
    } else if let Some(rest) = from.strip_prefix("endpoint:") {
        EdgeSource::Endpoint(rest.to_string())
    } else {
        EdgeSource::Node(from.to_string())
    }
}

pub fn parse_edge_target(to: &str) -> Option<(String, String)> {
    let dot_pos = to.find('.')?;
    let node = &to[..dot_pos];
    let input = &to[dot_pos + 1..];
    if node.is_empty() || input.is_empty() {
        return None;
    }
    Some((node.to_string(), input.to_string()))
}

#[derive(Debug, Error)]
pub enum OzzyTomlParseError {
    #[error("failed to parse ozzy.toml: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("failed to parse type definition at {location}: {source}")]
    TypeDefinition {
        location: String,
        #[source]
        source: TypeParseError,
    },
    #[error("failed to parse type reference at {location}: {source}")]
    TypeReference {
        location: String,
        #[source]
        source: TypeParseError,
    },
    #[error("duplicate type definition '{name}'")]
    DuplicateTypeDefinition { name: String },
}

#[derive(Debug, Clone)]
pub struct ValidationError {
    pub location: String,
    pub message: String,
    pub suggestion: Option<String>,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.location, self.message)?;
        if let Some(suggestion) = &self.suggestion {
            write!(f, " (did you mean \"{}\"?)", suggestion)?;
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct RawOzzyToml {
    project: ProjectSection,
    #[serde(default)]
    git: Option<GitSection>,
    #[serde(default)]
    remote: Option<RemoteSection>,
    #[serde(default)]
    types: HashMap<String, String>,
    #[serde(default)]
    environments: HashMap<String, EnvironmentDef>,
    #[serde(default)]
    transforms: HashMap<String, RawTransformDef>,
    #[serde(default)]
    endpoints: HashMap<String, EndpointDef>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawTransformDef {
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    command: Option<String>,
    environment: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    inputs: HashMap<String, RawTypedPort>,
    #[serde(default)]
    outputs: HashMap<String, RawTypedPort>,
    #[serde(default)]
    params: HashMap<String, ParamDef>,
    #[serde(default)]
    network: bool,
    #[serde(default)]
    secrets: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawTypedPort {
    #[serde(rename = "type")]
    ty: String,
    #[serde(default)]
    description: Option<String>,
}

impl OzzyToml {
    pub fn parse(s: &str) -> Result<Self, OzzyTomlParseError> {
        let raw: RawOzzyToml = toml::from_str(s)?;

        let mut types = TypeDefinitions::default();
        for (name, expr) in raw.types {
            let expr =
                parse_type_expr(&expr).map_err(|source| OzzyTomlParseError::TypeDefinition {
                    location: format!("types.{}", name),
                    source,
                })?;
            types
                .insert(TypeDefinition::new(name.clone(), expr))
                .map_err(|_| OzzyTomlParseError::DuplicateTypeDefinition { name })?;
        }

        let transforms = raw
            .transforms
            .into_iter()
            .map(|(name, raw_transform)| {
                let inputs = parse_typed_port_set(
                    &raw_transform.inputs,
                    format!("transforms.{}.inputs", name),
                )?;
                let outputs = parse_typed_port_set(
                    &raw_transform.outputs,
                    format!("transforms.{}.outputs", name),
                )?;
                Ok((
                    name,
                    TransformDef {
                        source: raw_transform.source,
                        command: raw_transform.command,
                        environment: raw_transform.environment,
                        description: raw_transform.description,
                        inputs,
                        outputs,
                        params: raw_transform.params,
                        network: raw_transform.network,
                        secrets: raw_transform.secrets,
                    },
                ))
            })
            .collect::<Result<HashMap<_, _>, OzzyTomlParseError>>()?;

        Ok(Self {
            project: raw.project,
            git: raw.git,
            remote: raw.remote,
            types,
            environments: raw.environments,
            transforms,
            endpoints: raw.endpoints,
        })
    }

    pub fn validate(&self) -> Vec<ValidationError> {
        let mut errors = Vec::new();

        self.validate_names(&mut errors);
        self.validate_types(&mut errors);
        self.validate_environments(&mut errors);
        self.validate_transforms(&mut errors);
        self.validate_endpoints(&mut errors);

        errors
    }

    fn validate_names(&self, errors: &mut Vec<ValidationError>) {
        if !is_valid_name(&self.project.name) {
            errors.push(ValidationError {
                location: "project.name".to_string(),
                message: format!(
                    "Invalid name \"{}\". Names must match [a-zA-Z0-9_-]+.",
                    self.project.name
                ),
                suggestion: None,
            });
        }
        if !is_valid_name(&self.project.owner) {
            errors.push(ValidationError {
                location: "project.owner".to_string(),
                message: format!(
                    "Invalid owner \"{}\". Names must match [a-zA-Z0-9_-]+.",
                    self.project.owner
                ),
                suggestion: None,
            });
        }
        for name in self.types.types.keys() {
            if !is_valid_name(name) {
                errors.push(ValidationError {
                    location: format!("types.{}", name),
                    message: format!(
                        "Invalid type name \"{}\". Local type names must match [a-zA-Z0-9_-]+.",
                        name
                    ),
                    suggestion: None,
                });
            }
        }
        for name in self.environments.keys() {
            if !is_valid_name(name) {
                errors.push(ValidationError {
                    location: format!("environments.{}", name),
                    message: format!(
                        "Invalid environment name \"{}\". Names must match [a-zA-Z0-9_-]+.",
                        name
                    ),
                    suggestion: None,
                });
            }
        }
        for (name, transform) in &self.transforms {
            if !is_valid_name(name) {
                errors.push(ValidationError {
                    location: format!("transforms.{}", name),
                    message: format!(
                        "Invalid transform name \"{}\". Names must match [a-zA-Z0-9_-]+.",
                        name
                    ),
                    suggestion: None,
                });
            }
            for input_name in transform.inputs.ports.keys() {
                if !is_valid_name(input_name) {
                    errors.push(ValidationError {
                        location: format!("transforms.{}.inputs.{}", name, input_name),
                        message: format!(
                            "Invalid input port name \"{}\". Names must match [a-zA-Z0-9_-]+.",
                            input_name
                        ),
                        suggestion: None,
                    });
                }
            }
            for output_name in transform.outputs.ports.keys() {
                if !is_valid_name(output_name) {
                    errors.push(ValidationError {
                        location: format!("transforms.{}.outputs.{}", name, output_name),
                        message: format!(
                            "Invalid output port name \"{}\". Names must match [a-zA-Z0-9_-]+.",
                            output_name
                        ),
                        suggestion: None,
                    });
                }
            }
            for param_name in transform.params.keys() {
                if !is_valid_name(param_name) {
                    errors.push(ValidationError {
                        location: format!("transforms.{}.params.{}", name, param_name),
                        message: format!(
                            "Invalid param name \"{}\". Names must match [a-zA-Z0-9_-]+.",
                            param_name
                        ),
                        suggestion: None,
                    });
                } else if param_name.contains('-') {
                    errors.push(ValidationError {
                        location: format!("transforms.{}.params.{}", name, param_name),
                        message: format!(
                            "Param name \"{}\" contains hyphens. Use underscores instead — hyphens produce invalid shell env var names.",
                            param_name
                        ),
                        suggestion: Some(param_name.replace('-', "_")),
                    });
                }
            }
        }
        for (name, endpoint) in &self.endpoints {
            if !is_valid_name(name) {
                errors.push(ValidationError {
                    location: format!("endpoints.{}", name),
                    message: format!(
                        "Invalid endpoint name \"{}\". Names must match [a-zA-Z0-9_-]+.",
                        name
                    ),
                    suggestion: None,
                });
            }
            for node_name in endpoint.nodes.keys() {
                if !is_valid_name(node_name) {
                    errors.push(ValidationError {
                        location: format!("endpoints.{}.nodes.{}", name, node_name),
                        message: format!(
                            "Invalid node name \"{}\". Names must match [a-zA-Z0-9_-]+.",
                            node_name
                        ),
                        suggestion: None,
                    });
                }
            }
            for param_name in endpoint.params.keys() {
                if !is_valid_name(param_name) {
                    errors.push(ValidationError {
                        location: format!("endpoints.{}.params.{}", name, param_name),
                        message: format!(
                            "Invalid endpoint param name \"{}\". Names must match [a-zA-Z0-9_-]+.",
                            param_name
                        ),
                        suggestion: None,
                    });
                }
            }
        }
    }

    fn validate_types(&self, errors: &mut Vec<ValidationError>) {
        if let Err(err) = self.types.validate_all() {
            errors.push(ValidationError {
                location: "types".to_string(),
                message: err.to_string(),
                suggestion: None,
            });
        }
    }

    fn validate_environments(&self, errors: &mut Vec<ValidationError>) {
        for (name, env) in &self.environments {
            if env.tier().is_none() {
                errors.push(ValidationError {
                    location: format!("environments.{}", name),
                    message:
                        "Environment must specify exactly one of: (base + lockfile), dockerfile, or image."
                            .to_string(),
                    suggestion: None,
                });
            }
        }
    }

    fn validate_transforms(&self, errors: &mut Vec<ValidationError>) {
        let env_names: Vec<&str> = self.environments.keys().map(|s| s.as_str()).collect();

        for (name, transform) in &self.transforms {
            match (&transform.source, &transform.command) {
                (Some(_), Some(_)) => errors.push(ValidationError {
                    location: format!("transforms.{}", name),
                    message: "Transform must have exactly one of `source` or `command`, not both."
                        .to_string(),
                    suggestion: None,
                }),
                (None, None) => errors.push(ValidationError {
                    location: format!("transforms.{}", name),
                    message: "Transform must have either `source` or `command`.".to_string(),
                    suggestion: None,
                }),
                _ => {}
            }

            if !self.environments.contains_key(&transform.environment) {
                errors.push(ValidationError {
                    location: format!("transforms.{}.environment", name),
                    message: format!("Environment \"{}\" not found.", transform.environment),
                    suggestion: suggest_name(&transform.environment, env_names.iter().copied()),
                });
            }

            if transform.outputs.ports.len() != 1 {
                errors.push(ValidationError {
                    location: format!("transforms.{}.outputs", name),
                    message: format!(
                        "Transform must declare exactly one output port for now; found {}.",
                        transform.outputs.ports.len()
                    ),
                    suggestion: None,
                });
            }

            for (port_name, port) in &transform.inputs.ports {
                self.validate_port_type_ref(
                    &format!("transforms.{}.inputs.{}", name, port_name),
                    &port.ty,
                    errors,
                );
            }

            for (port_name, port) in &transform.outputs.ports {
                self.validate_port_type_ref(
                    &format!("transforms.{}.outputs.{}", name, port_name),
                    &port.ty,
                    errors,
                );
            }

            for (param_name, param) in &transform.params {
                if !matches!(param.type_.as_str(), "float" | "int" | "string" | "bool") {
                    errors.push(ValidationError {
                        location: format!("transforms.{}.params.{}.type", name, param_name),
                        message: format!(
                            "Invalid param type \"{}\". Must be float, int, string, or bool.",
                            param.type_
                        ),
                        suggestion: None,
                    });
                }
            }
        }
    }

    fn validate_port_type_ref(
        &self,
        location: &str,
        type_ref: &ozzy_types::syntax::TypeRefExpr,
        errors: &mut Vec<ValidationError>,
    ) {
        if let Some(version) = &type_ref.version {
            if BuiltinType::parse(&type_ref.name).is_some() {
                errors.push(ValidationError {
                    location: location.to_string(),
                    message: format!(
                        "Builtin type \"{}\" cannot be version-pinned as \"{}\".",
                        type_ref.name, version
                    ),
                    suggestion: None,
                });
                return;
            }
            return;
        }

        if type_ref.version.is_none() {
            if self.types.get(&type_ref.name).is_some() {
                return;
            }
            if BuiltinType::parse(&type_ref.name).is_some() {
                errors.push(ValidationError {
                    location: location.to_string(),
                    message: format!(
                        "Builtin type \"{}\" cannot be used directly on transform ports; define it in [types] and reference that named type instead.",
                        type_ref.name
                    ),
                    suggestion: None,
                });
                return;
            }
            errors.push(ValidationError {
                location: location.to_string(),
                message: format!(
                    "Type reference \"{}\" is not defined in [types] and is not a published version-pinned type reference.",
                    type_ref.name
                ),
                suggestion: suggest_name(
                    &type_ref.name,
                    self.types.types.keys().map(String::as_str),
                ),
            });
        }
    }

    fn validate_endpoints(&self, errors: &mut Vec<ValidationError>) {
        let transform_names: Vec<&str> = self.transforms.keys().map(|s| s.as_str()).collect();

        for (endpoint_name, endpoint) in &self.endpoints {
            let node_names: Vec<&str> = endpoint.nodes.keys().map(|s| s.as_str()).collect();
            let mut covered_inputs: HashMap<(String, String), usize> = HashMap::new();

            for (node_name, node) in &endpoint.nodes {
                if !self.transforms.contains_key(&node.transform) {
                    errors.push(ValidationError {
                        location: format!(
                            "endpoints.{}.nodes.{}.transform",
                            endpoint_name, node_name
                        ),
                        message: format!("Transform \"{}\" not found.", node.transform),
                        suggestion: suggest_name(&node.transform, transform_names.iter().copied()),
                    });
                }
            }

            for (idx, edge) in endpoint.edges.iter().enumerate() {
                let edge_loc = format!("endpoints.{}.edges[{}]", endpoint_name, idx);

                match parse_edge_target(&edge.to) {
                    Some((node_name, input_name)) => {
                        if !endpoint.nodes.contains_key(&node_name) {
                            errors.push(ValidationError {
                                location: format!("{}.to", edge_loc),
                                message: format!(
                                    "Target node \"{}\" not found in endpoint.",
                                    node_name
                                ),
                                suggestion: suggest_name(&node_name, node_names.iter().copied()),
                            });
                        } else if let Some(transform) =
                            self.transforms.get(&endpoint.nodes[&node_name].transform)
                        {
                            if !transform.inputs.ports.contains_key(&input_name) {
                                let input_names: Vec<&str> =
                                    transform.inputs.ports.keys().map(String::as_str).collect();
                                errors.push(ValidationError {
                                    location: format!("{}.to", edge_loc),
                                    message: format!(
                                        "Input \"{}\" not declared on transform \"{}\".",
                                        input_name, endpoint.nodes[&node_name].transform
                                    ),
                                    suggestion: suggest_name(
                                        &input_name,
                                        input_names.iter().copied(),
                                    ),
                                });
                            }
                            *covered_inputs.entry((node_name, input_name)).or_insert(0) += 1;
                        }
                    }
                    None => errors.push(ValidationError {
                        location: format!("{}.to", edge_loc),
                        message: format!(
                            "Invalid edge target \"{}\". Must be \"node_name.input_name\".",
                            edge.to
                        ),
                        suggestion: None,
                    }),
                }

                let source = parse_edge_source(&edge.from);
                let empty_ref = match &source {
                    EdgeSource::Data(r) | EdgeSource::Collection(r) | EdgeSource::Endpoint(r) => {
                        r.is_empty()
                    }
                    EdgeSource::Node(r) => r.is_empty(),
                };
                if empty_ref {
                    errors.push(ValidationError {
                        location: format!("{}.from", edge_loc),
                        message: format!(
                            "Empty edge source \"{}\". Must specify a reference after the prefix.",
                            edge.from
                        ),
                        suggestion: None,
                    });
                }

                match source {
                    EdgeSource::Node(node_ref) => {
                        if !node_ref.is_empty() && !endpoint.nodes.contains_key(&node_ref) {
                            errors.push(ValidationError {
                                location: format!("{}.from", edge_loc),
                                message: format!(
                                    "Source node \"{}\" not found in endpoint.",
                                    node_ref
                                ),
                                suggestion: suggest_name(&node_ref, node_names.iter().copied()),
                            });
                        }
                    }
                    EdgeSource::Data(_) | EdgeSource::Collection(_) => {}
                    EdgeSource::Endpoint(ref_str) => {
                        if ref_str.contains('/') {
                            let pin_valid = ref_str
                                .split_once('@')
                                .is_some_and(|(_, pin)| !pin.is_empty());
                            if !pin_valid {
                                errors.push(ValidationError {
                                    location: format!("{}.from", edge_loc),
                                    message: format!(
                                        "Cross-project endpoint ref \"{}\" must include @sha_or_tag.",
                                        ref_str
                                    ),
                                    suggestion: None,
                                });
                            }
                        }
                    }
                }
            }

            for (node_name, node) in &endpoint.nodes {
                if let Some(transform) = self.transforms.get(&node.transform) {
                    for input_name in transform.inputs.ports.keys() {
                        let key = (node_name.clone(), input_name.clone());
                        match covered_inputs.get(&key) {
                            None => errors.push(ValidationError {
                                location: format!(
                                    "endpoints.{}.nodes.{}",
                                    endpoint_name, node_name
                                ),
                                message: format!("Input \"{}\" has no incoming edge.", input_name),
                                suggestion: None,
                            }),
                            Some(&count) if count > 1 => errors.push(ValidationError {
                                location: format!(
                                    "endpoints.{}.nodes.{}",
                                    endpoint_name, node_name
                                ),
                                message: format!(
                                    "Input \"{}\" has {} incoming edges (expected exactly 1).",
                                    input_name, count
                                ),
                                suggestion: None,
                            }),
                            _ => {}
                        }
                    }
                }
            }

            self.validate_no_cycles(endpoint_name, endpoint, errors);

            const RESERVED_PARAM_NAMES: &[&str] = &["ref", "format"];
            let mut bind_targets: HashMap<String, Vec<String>> = HashMap::new();
            for (param_name, param) in &endpoint.params {
                if RESERVED_PARAM_NAMES.contains(&param_name.as_str()) {
                    errors.push(ValidationError {
                        location: format!("endpoints.{}.params.{}", endpoint_name, param_name),
                        message: format!(
                            "Parameter name \"{}\" is reserved (conflicts with API query parameter).",
                            param_name
                        ),
                        suggestion: None,
                    });
                }

                let Some((bind_node, bind_param)) = param.binds.split_once('.') else {
                    errors.push(ValidationError {
                        location: format!(
                            "endpoints.{}.params.{}.binds",
                            endpoint_name, param_name
                        ),
                        message: format!(
                            "Invalid bind \"{}\" — must be \"node_name.param_name\".",
                            param.binds
                        ),
                        suggestion: None,
                    });
                    continue;
                };

                if !endpoint.nodes.contains_key(bind_node) {
                    errors.push(ValidationError {
                        location: format!(
                            "endpoints.{}.params.{}.binds",
                            endpoint_name, param_name
                        ),
                        message: format!(
                            "Bind target node \"{}\" not found in endpoint.",
                            bind_node
                        ),
                        suggestion: suggest_name(bind_node, node_names.iter().copied()),
                    });
                } else if let Some(transform) =
                    self.transforms.get(&endpoint.nodes[bind_node].transform)
                {
                    if !transform.params.contains_key(bind_param) {
                        let param_names: Vec<&str> =
                            transform.params.keys().map(String::as_str).collect();
                        errors.push(ValidationError {
                            location: format!(
                                "endpoints.{}.params.{}.binds",
                                endpoint_name, param_name
                            ),
                            message: format!(
                                "Bind target param \"{}\" not found on transform \"{}\".",
                                bind_param, endpoint.nodes[bind_node].transform
                            ),
                            suggestion: suggest_name(bind_param, param_names.iter().copied()),
                        });
                    }
                }

                bind_targets
                    .entry(param.binds.clone())
                    .or_default()
                    .push(param_name.clone());
            }

            for (target, params) in bind_targets {
                if params.len() > 1 {
                    errors.push(ValidationError {
                        location: format!("endpoints.{}.params", endpoint_name),
                        message: format!(
                            "Multiple parameters bind to \"{}\": [{}]. Each node parameter must have at most one bind.",
                            target,
                            params.join(", ")
                        ),
                        suggestion: None,
                    });
                }
            }
        }
    }

    fn validate_no_cycles(
        &self,
        endpoint_name: &str,
        endpoint: &EndpointDef,
        errors: &mut Vec<ValidationError>,
    ) {
        let mut in_degree: HashMap<String, usize> = HashMap::new();
        let mut successors: HashMap<String, Vec<String>> = HashMap::new();

        for node_name in endpoint.nodes.keys() {
            in_degree.entry(node_name.clone()).or_insert(0);
            successors.entry(node_name.clone()).or_default();
        }

        for edge in &endpoint.edges {
            if let EdgeSource::Node(src_node) = parse_edge_source(&edge.from) {
                if let Some((target_node, _)) = parse_edge_target(&edge.to) {
                    if endpoint.nodes.contains_key(&src_node)
                        && endpoint.nodes.contains_key(&target_node)
                    {
                        successors
                            .entry(src_node)
                            .or_default()
                            .push(target_node.clone());
                        *in_degree.entry(target_node).or_insert(0) += 1;
                    }
                }
            }
        }

        let mut queue: VecDeque<String> = in_degree
            .iter()
            .filter(|(_, degree)| **degree == 0)
            .map(|(name, _)| name.clone())
            .collect();
        let mut visited = 0usize;

        while let Some(node) = queue.pop_front() {
            visited += 1;
            if let Some(children) = successors.get(&node) {
                for child in children {
                    if let Some(degree) = in_degree.get_mut(child) {
                        *degree -= 1;
                        if *degree == 0 {
                            queue.push_back(child.clone());
                        }
                    }
                }
            }
        }

        if visited < endpoint.nodes.len() {
            errors.push(ValidationError {
                location: format!("endpoints.{}", endpoint_name),
                message: "Endpoint DAG contains a cycle.".to_string(),
                suggestion: None,
            });
        }
    }
}

fn parse_typed_port_set(
    raw_ports: &HashMap<String, RawTypedPort>,
    location: String,
) -> Result<TypedPortSet, OzzyTomlParseError> {
    let mut ports = TypedPortSet::default();
    for (name, raw_port) in raw_ports {
        let type_ref =
            parse_type_ref(&raw_port.ty).map_err(|source| OzzyTomlParseError::TypeReference {
                location: format!("{}.{}.type", location, name),
                source,
            })?;
        ports.insert(
            name.clone(),
            TypedPort {
                ty: type_ref,
                description: raw_port.description.clone(),
            },
        );
    }
    Ok(ports)
}

fn suggest_name<'a>(target: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let target_lower = target.to_lowercase();
    candidates
        .into_iter()
        .filter_map(|candidate| {
            let distance = edit_distance(&target_lower, &candidate.to_lowercase());
            if distance <= 3 {
                Some((candidate.to_string(), distance))
            } else {
                None
            }
        })
        .min_by_key(|(_, distance)| *distance)
        .map(|(name, _)| name)
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let m = a.len();
    let n = b.len();
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in 0..=m {
        dp[i][0] = i;
    }
    for j in 0..=n {
        dp[0][j] = j;
    }
    for i in 1..=m {
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            dp[i][j] = (dp[i - 1][j] + 1)
                .min(dp[i][j - 1] + 1)
                .min(dp[i - 1][j - 1] + cost);
        }
    }
    dp[m][n]
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_TOML: &str = r#"
[project]
name = "sapflux-analysis"
owner = "rileyleff"

[types]
WaterPotential = 'float64 & unit(value="MPa") & max(value=0)'
WaterPotentialRow = '{ species: string, wp: WaterPotential, date: date }'
RawWaterPotentialCsv = 'csv(delimiter=",", header=true) & table<WaterPotentialRow>'

[environments.scipy]
base = "ozzydb/python:3.12"
lockfile = "uv.lock"

[transforms.clean]
source = "transforms/clean.py:clean_fn"
environment = "scipy"

[transforms.clean.inputs.raw]
type = "RawWaterPotentialCsv"

[transforms.clean.outputs.result]
type = "RawWaterPotentialCsv"

[transforms.clean.params.threshold]
type = "float"

[endpoints.cleaned]
description = "Cleaned data"

[endpoints.cleaned.nodes]
clean = { transform = "clean" }

[[endpoints.cleaned.edges]]
from = "data:raw"
to = "clean.raw"
"#;

    #[test]
    fn parses_valid_v4_toml() {
        let doc = OzzyToml::parse(VALID_TOML).expect("valid v4 toml should parse");
        assert_eq!(doc.types.types.len(), 3);
        assert_eq!(doc.transforms["clean"].inputs.ports.len(), 1);
        assert_eq!(doc.transforms["clean"].outputs.ports.len(), 1);
        assert!(doc.validate().is_empty());
    }

    #[test]
    fn parse_rejects_invalid_type_definition() {
        let toml = r#"
[project]
name = "test"
owner = "user"

[types]
Broken = 'float64 & )'
"#;
        let err = OzzyToml::parse(toml).expect_err("invalid type expr should fail");
        assert!(err.to_string().contains("types.Broken"));
    }

    #[test]
    fn parse_rejects_invalid_port_type_ref() {
        let toml = r#"
[project]
name = "test"
owner = "user"

[environments.default]
base = "ozzydb/python:3.12"
lockfile = "uv.lock"

[transforms.t]
source = "t.py:run"
environment = "default"

[transforms.t.inputs.raw]
type = "float64 & max(value=0)"

[transforms.t.outputs.result]
type = "float64"
"#;
        let err = OzzyToml::parse(toml).expect_err("full type expr should be rejected in port ref");
        assert!(err.to_string().contains("transforms.t.inputs.raw.type"));
    }

    #[test]
    fn validate_requires_local_or_published_input_types() {
        let toml = r#"
[project]
name = "test"
owner = "user"

[environments.default]
base = "ozzydb/python:3.12"
lockfile = "uv.lock"

[transforms.t]
source = "t.py:run"
environment = "default"

[transforms.t.inputs.raw]
type = "UnknownLocalType"

[transforms.t.outputs.result]
type = "PublishedThing@1"
"#;
        let doc = OzzyToml::parse(toml).expect("toml should parse");
        let errors = doc.validate();
        assert!(
            errors
                .iter()
                .any(|err| err.location == "transforms.t.inputs.raw")
        );
    }

    #[test]
    fn validate_rejects_version_pinned_builtin_port_types() {
        let toml = r#"
[project]
name = "test"
owner = "user"

[environments.default]
base = "ozzydb/python:3.12"
lockfile = "uv.lock"

[transforms.t]
source = "t.py:run"
environment = "default"

[transforms.t.inputs.raw]
type = "parquet@1"

[transforms.t.outputs.result]
type = "parquet"
"#;
        let doc = OzzyToml::parse(toml).expect("toml should parse");
        let errors = doc.validate();
        assert!(
            errors
                .iter()
                .any(|err| err.location == "transforms.t.inputs.raw"
                    && err.message.contains("cannot be version-pinned"))
        );
    }

    #[test]
    fn validate_rejects_unversioned_builtin_port_types() {
        let toml = r#"
[project]
name = "test"
owner = "user"

[environments.default]
base = "ozzydb/python:3.12"
lockfile = "uv.lock"

[transforms.t]
source = "t.py:run"
environment = "default"

[transforms.t.inputs.raw]
type = "parquet"

[transforms.t.outputs.result]
type = "PublishedThing@1"
"#;
        let doc = OzzyToml::parse(toml).expect("toml should parse");
        let errors = doc.validate();
        assert!(errors.iter().any(|err| {
            err.location == "transforms.t.inputs.raw"
                && err
                    .message
                    .contains("cannot be used directly on transform ports")
        }));
    }

    #[test]
    fn validate_requires_single_output_port_for_now() {
        let toml = r#"
[project]
name = "test"
owner = "user"

[types]
Csv = 'csv(delimiter=",", header=true)'

[environments.default]
base = "ozzydb/python:3.12"
lockfile = "uv.lock"

[transforms.t]
source = "t.py:run"
environment = "default"

[transforms.t.inputs.raw]
type = "Csv"

[transforms.t.outputs.first]
type = "Csv"

[transforms.t.outputs.second]
type = "Csv"
"#;
        let doc = OzzyToml::parse(toml).expect("toml should parse");
        let errors = doc.validate();
        assert!(
            errors
                .iter()
                .any(|err| err.location == "transforms.t.outputs")
        );
    }

    #[test]
    fn validate_endpoint_inputs_against_typed_ports() {
        let toml = r#"
[project]
name = "test"
owner = "user"

[types]
Csv = 'csv(delimiter=",", header=true)'

[environments.default]
base = "ozzydb/python:3.12"
lockfile = "uv.lock"

[transforms.t]
source = "t.py:run"
environment = "default"

[transforms.t.inputs.expected]
type = "Csv"

[transforms.t.outputs.result]
type = "Csv"

[endpoints.ep]

[endpoints.ep.nodes]
n = { transform = "t" }

[[endpoints.ep.edges]]
from = "data:raw"
to = "n.wrong"
"#;
        let doc = OzzyToml::parse(toml).expect("toml should parse");
        let errors = doc.validate();
        assert!(
            errors
                .iter()
                .any(|err| err.location == "endpoints.ep.edges[0].to")
        );
    }
}
