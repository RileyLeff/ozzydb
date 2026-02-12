//! `ozzy.toml` specification parser and validator.
//!
//! The `ozzy.toml` file is the declarative heart of OzzyDB's compute plane.
//! It defines environments, transforms, and endpoints (DAGs of transforms
//! wired to data sources and each other).

use std::collections::{HashMap, VecDeque};

use serde::{Deserialize, Serialize};

// ============================================================================
// Name validation regex
// ============================================================================

/// Check that a name matches `[a-zA-Z0-9_-]+`.
fn is_valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

// ============================================================================
// Top-level document
// ============================================================================

/// Parsed `ozzy.toml` file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OzzyToml {
    pub project: ProjectSection,
    #[serde(default)]
    pub git: Option<GitSection>,
    #[serde(default)]
    pub remote: Option<RemoteSection>,
    #[serde(default)]
    pub environments: HashMap<String, EnvironmentDef>,
    #[serde(default)]
    pub transforms: HashMap<String, TransformDef>,
    #[serde(default)]
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

// ============================================================================
// Environments
// ============================================================================

/// Environment definition. Exactly one of three tiers:
/// - `base` + `lockfile`: OzzyDB base image with user lockfile
/// - `dockerfile`: Custom Dockerfile path
/// - `image`: Pre-built container image reference
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
    /// Returns which tier this environment uses, or None if invalid.
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

#[derive(Debug, Clone)]
pub enum EnvironmentTier {
    BaseLockfile { base: String, lockfile: String },
    Dockerfile { dockerfile: String },
    Prebuilt { image: String },
}

// ============================================================================
// Transforms
// ============================================================================

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
    pub inputs: HashMap<String, String>,
    #[serde(default = "default_output")]
    pub output: String,
    #[serde(default)]
    pub params: HashMap<String, ParamDef>,
    #[serde(default)]
    pub output_schema: Option<OutputSchemaDef>,
    #[serde(default)]
    pub network: bool,
    #[serde(default)]
    pub secrets: Vec<String>,
}

fn default_output() -> String {
    "parquet".to_string()
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
pub struct OutputSchemaDef {
    pub columns: Vec<ColumnDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnDef {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(default)]
    pub constraints: Option<ConstraintsDef>,
    #[serde(default)]
    pub unit: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstraintsDef {
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
}

// ============================================================================
// Endpoints
// ============================================================================

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
pub struct NodeDef {
    pub transform: String,
    #[serde(default)]
    pub params: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub machine: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeDef {
    pub from: String,
    pub to: String,
}

// ============================================================================
// Parsing
// ============================================================================

impl OzzyToml {
    /// Parse an `ozzy.toml` from a string.
    pub fn parse(s: &str) -> std::result::Result<Self, toml::de::Error> {
        toml::from_str(s)
    }

    /// Serialize back to TOML string.
    pub fn to_toml_string(&self) -> std::result::Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }
}

// ============================================================================
// Validation
// ============================================================================

/// A validation error with location context.
#[derive(Debug, Clone)]
pub struct ValidationError {
    /// Dot-separated path (e.g., "transforms.qc.environment").
    pub location: String,
    /// Human-readable error message.
    pub message: String,
    /// Optional "did you mean?" suggestion.
    pub suggestion: Option<String>,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.location, self.message)?;
        if let Some(sug) = &self.suggestion {
            write!(f, " (did you mean \"{}\"?)", sug)?;
        }
        Ok(())
    }
}

/// Find the closest match in `candidates` to `target` using edit distance.
fn suggest_name<'a>(target: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let target_lower = target.to_lowercase();
    candidates
        .into_iter()
        .filter_map(|c| {
            let dist = edit_distance(&target_lower, &c.to_lowercase());
            if dist <= 3 {
                Some((c.to_string(), dist))
            } else {
                None
            }
        })
        .min_by_key(|(_, d)| *d)
        .map(|(name, _)| name)
}

/// Simple Levenshtein edit distance.
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

/// Parse an edge `from` source. Returns the parsed variant.
#[derive(Debug, Clone, PartialEq)]
pub enum EdgeSource {
    /// `data:<name>`
    Data(String),
    /// `collection:<name>`
    Collection(String),
    /// `endpoint:<ref>` (possibly cross-project with `owner/project/name@ref`)
    Endpoint(String),
    /// Bare node name within the same endpoint.
    Node(String),
}

fn parse_edge_source(from: &str) -> EdgeSource {
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

/// Parse an edge `to` target. Returns `(node_name, input_name)`.
fn parse_edge_target(to: &str) -> Option<(String, String)> {
    let dot_pos = to.find('.')?;
    let node = &to[..dot_pos];
    let input = &to[dot_pos + 1..];
    if node.is_empty() || input.is_empty() {
        return None;
    }
    Some((node.to_string(), input.to_string()))
}

impl OzzyToml {
    /// Validate the document, returning all errors found.
    pub fn validate(&self) -> Vec<ValidationError> {
        let mut errors = Vec::new();

        self.validate_names(&mut errors);
        self.validate_environments(&mut errors);
        self.validate_transforms(&mut errors);
        self.validate_endpoints(&mut errors);

        errors
    }

    /// Rule 1: All names match `[a-zA-Z0-9_-]+`.
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
        for (name, t) in &self.transforms {
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
            for input_name in t.inputs.keys() {
                if !is_valid_name(input_name) {
                    errors.push(ValidationError {
                        location: format!("transforms.{}.inputs.{}", name, input_name),
                        message: format!(
                            "Invalid input name \"{}\". Names must match [a-zA-Z0-9_-]+.",
                            input_name
                        ),
                        suggestion: None,
                    });
                }
            }
            for param_name in t.params.keys() {
                if !is_valid_name(param_name) {
                    errors.push(ValidationError {
                        location: format!("transforms.{}.params.{}", name, param_name),
                        message: format!(
                            "Invalid param name \"{}\". Names must match [a-zA-Z0-9_-]+.",
                            param_name
                        ),
                        suggestion: None,
                    });
                }
            }
        }
        for (name, ep) in &self.endpoints {
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
            for node_name in ep.nodes.keys() {
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
            for param_name in ep.params.keys() {
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

    /// Rule 2: Environment refs exist. Rule on environment tier exclusivity.
    fn validate_environments(&self, errors: &mut Vec<ValidationError>) {
        for (name, env) in &self.environments {
            if env.tier().is_none() {
                errors.push(ValidationError {
                    location: format!("environments.{}", name),
                    message: "Environment must specify exactly one of: (base + lockfile), dockerfile, or image.".to_string(),
                    suggestion: None,
                });
            }
        }
    }

    /// Rules 2 (env refs), 3 (transform exclusivity).
    fn validate_transforms(&self, errors: &mut Vec<ValidationError>) {
        let env_names: Vec<&str> = self.environments.keys().map(|s| s.as_str()).collect();

        for (name, t) in &self.transforms {
            // Rule 3: source XOR command
            match (&t.source, &t.command) {
                (Some(_), Some(_)) => {
                    errors.push(ValidationError {
                        location: format!("transforms.{}", name),
                        message: "Transform must have exactly one of `source` or `command`, not both.".to_string(),
                        suggestion: None,
                    });
                }
                (None, None) => {
                    errors.push(ValidationError {
                        location: format!("transforms.{}", name),
                        message: "Transform must have either `source` or `command`.".to_string(),
                        suggestion: None,
                    });
                }
                _ => {}
            }

            // Rule 2: environment exists
            if !self.environments.contains_key(&t.environment) {
                errors.push(ValidationError {
                    location: format!("transforms.{}.environment", name),
                    message: format!(
                        "Environment \"{}\" not found.",
                        t.environment
                    ),
                    suggestion: suggest_name(&t.environment, env_names.iter().copied()),
                });
            }

            // Validate param types
            for (pname, pdef) in &t.params {
                if !matches!(pdef.type_.as_str(), "float" | "int" | "string" | "bool") {
                    errors.push(ValidationError {
                        location: format!("transforms.{}.params.{}.type", name, pname),
                        message: format!(
                            "Invalid param type \"{}\". Must be float, int, string, or bool.",
                            pdef.type_
                        ),
                        suggestion: None,
                    });
                }
            }
        }
    }

    /// Rules 4-11: Endpoint validation.
    fn validate_endpoints(&self, errors: &mut Vec<ValidationError>) {
        let transform_names: Vec<&str> = self.transforms.keys().map(|s| s.as_str()).collect();

        for (ep_name, ep) in &self.endpoints {
            let node_names: Vec<&str> = ep.nodes.keys().map(|s| s.as_str()).collect();

            // Rule 4: node transform refs exist
            for (node_name, node) in &ep.nodes {
                if !self.transforms.contains_key(&node.transform) {
                    errors.push(ValidationError {
                        location: format!("endpoints.{}.nodes.{}.transform", ep_name, node_name),
                        message: format!(
                            "Transform \"{}\" not found.",
                            node.transform
                        ),
                        suggestion: suggest_name(&node.transform, transform_names.iter().copied()),
                    });
                }
            }

            // Rule 5, 6, 7: Edge validation
            // Track which (node, input) pairs have been covered
            let mut covered_inputs: HashMap<(String, String), usize> = HashMap::new();

            for (idx, edge) in ep.edges.iter().enumerate() {
                let edge_loc = format!("endpoints.{}.edges[{}]", ep_name, idx);

                // Rule 5: Parse and validate `to` target
                match parse_edge_target(&edge.to) {
                    Some((node_name, input_name)) => {
                        if !ep.nodes.contains_key(&node_name) {
                            errors.push(ValidationError {
                                location: format!("{}.to", edge_loc),
                                message: format!(
                                    "Target node \"{}\" not found in endpoint.",
                                    node_name
                                ),
                                suggestion: suggest_name(&node_name, node_names.iter().copied()),
                            });
                        } else {
                            // Check that the input is declared on the transform
                            let node = &ep.nodes[&node_name];
                            if let Some(transform) = self.transforms.get(&node.transform) {
                                if !transform.inputs.contains_key(&input_name) {
                                    let input_names: Vec<&str> = transform.inputs.keys().map(|s| s.as_str()).collect();
                                    errors.push(ValidationError {
                                        location: format!("{}.to", edge_loc),
                                        message: format!(
                                            "Input \"{}\" not declared on transform \"{}\".",
                                            input_name, node.transform
                                        ),
                                        suggestion: suggest_name(&input_name, input_names.iter().copied()),
                                    });
                                }
                            }
                            // Track coverage
                            let key = (node_name.clone(), input_name.clone());
                            *covered_inputs.entry(key).or_insert(0) += 1;
                        }
                    }
                    None => {
                        errors.push(ValidationError {
                            location: format!("{}.to", edge_loc),
                            message: format!(
                                "Invalid edge target \"{}\". Must be \"node_name.input_name\".",
                                edge.to
                            ),
                            suggestion: None,
                        });
                    }
                }

                // Rule 6: Parse and validate `from` source
                let source = parse_edge_source(&edge.from);

                // Reject empty refs (e.g. "data:", "collection:", "endpoint:")
                let is_empty_ref = match &source {
                    EdgeSource::Data(r) | EdgeSource::Collection(r) | EdgeSource::Endpoint(r) => r.is_empty(),
                    EdgeSource::Node(r) => r.is_empty(),
                };
                if is_empty_ref {
                    errors.push(ValidationError {
                        location: format!("{}.from", edge_loc),
                        message: format!(
                            "Empty edge source \"{}\". Must specify a reference after the prefix.",
                            edge.from
                        ),
                        suggestion: None,
                    });
                }

                match &source {
                    EdgeSource::Node(node_ref) => {
                        if !node_ref.is_empty() && !ep.nodes.contains_key(node_ref.as_str()) {
                            errors.push(ValidationError {
                                location: format!("{}.from", edge_loc),
                                message: format!(
                                    "Source node \"{}\" not found in endpoint.",
                                    node_ref
                                ),
                                suggestion: suggest_name(node_ref, node_names.iter().copied()),
                            });
                        }
                    }
                    EdgeSource::Data(_) | EdgeSource::Collection(_) => {
                        // Data and collection refs are validated at runtime (they may not
                        // exist in the toml - they're server-side resources).
                    }
                    EdgeSource::Endpoint(ref_str) => {
                        // Rule 10: Cross-project refs must be pinned
                        if ref_str.contains('/') && !ref_str.contains('@') {
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

            // Rule 7: Input coverage — every node input must have exactly one edge
            for (node_name, node) in &ep.nodes {
                if let Some(transform) = self.transforms.get(&node.transform) {
                    for input_name in transform.inputs.keys() {
                        let key = (node_name.clone(), input_name.clone());
                        match covered_inputs.get(&key) {
                            None => {
                                errors.push(ValidationError {
                                    location: format!(
                                        "endpoints.{}.nodes.{}", ep_name, node_name
                                    ),
                                    message: format!(
                                        "Input \"{}\" has no incoming edge.",
                                        input_name
                                    ),
                                    suggestion: None,
                                });
                            }
                            Some(&count) if count > 1 => {
                                errors.push(ValidationError {
                                    location: format!(
                                        "endpoints.{}.nodes.{}", ep_name, node_name
                                    ),
                                    message: format!(
                                        "Input \"{}\" has {} incoming edges (expected exactly 1).",
                                        input_name, count
                                    ),
                                    suggestion: None,
                                });
                            }
                            _ => {}
                        }
                    }
                }
            }

            // Rule 8: No cycles (Kahn's algorithm on the node graph)
            self.validate_no_cycles(ep_name, ep, errors);

            // Rule 9: Param binds
            for (param_name, param) in &ep.params {
                let parts: Vec<&str> = param.binds.splitn(2, '.').collect();
                if parts.len() != 2 {
                    errors.push(ValidationError {
                        location: format!("endpoints.{}.params.{}.binds", ep_name, param_name),
                        message: format!(
                            "Invalid bind \"{}\" — must be \"node_name.param_name\".",
                            param.binds
                        ),
                        suggestion: None,
                    });
                    continue;
                }
                let (bind_node, bind_param) = (parts[0], parts[1]);
                if !ep.nodes.contains_key(bind_node) {
                    errors.push(ValidationError {
                        location: format!("endpoints.{}.params.{}.binds", ep_name, param_name),
                        message: format!(
                            "Bind target node \"{}\" not found in endpoint.",
                            bind_node
                        ),
                        suggestion: suggest_name(bind_node, node_names.iter().copied()),
                    });
                } else {
                    let node = &ep.nodes[bind_node];
                    if let Some(transform) = self.transforms.get(&node.transform) {
                        if !transform.params.contains_key(bind_param) {
                            let param_names: Vec<&str> =
                                transform.params.keys().map(|s| s.as_str()).collect();
                            errors.push(ValidationError {
                                location: format!(
                                    "endpoints.{}.params.{}.binds", ep_name, param_name
                                ),
                                message: format!(
                                    "Bind target param \"{}\" not found on transform \"{}\".",
                                    bind_param, node.transform
                                ),
                                suggestion: suggest_name(
                                    bind_param,
                                    param_names.iter().copied(),
                                ),
                            });
                        }
                    }
                }
            }
        }
    }

    /// Rule 8: No cycles in the endpoint's node graph.
    fn validate_no_cycles(
        &self,
        ep_name: &str,
        ep: &EndpointDef,
        errors: &mut Vec<ValidationError>,
    ) {
        // Build adjacency using owned Strings to avoid lifetime issues
        let mut in_degree: HashMap<String, usize> = HashMap::new();
        let mut successors: HashMap<String, Vec<String>> = HashMap::new();

        for node_name in ep.nodes.keys() {
            in_degree.entry(node_name.clone()).or_insert(0);
            successors.entry(node_name.clone()).or_default();
        }

        for edge in &ep.edges {
            let source = parse_edge_source(&edge.from);
            if let EdgeSource::Node(src_node) = source {
                if let Some((tgt_node, _)) = parse_edge_target(&edge.to) {
                    if ep.nodes.contains_key(&src_node) && ep.nodes.contains_key(&tgt_node) {
                        successors
                            .entry(src_node)
                            .or_default()
                            .push(tgt_node.clone());
                        *in_degree.entry(tgt_node).or_insert(0) += 1;
                    }
                }
            }
        }

        // Kahn's algorithm
        let mut queue: VecDeque<String> = in_degree
            .iter()
            .filter(|(_, deg)| **deg == 0)
            .map(|(name, _)| name.clone())
            .collect();

        let mut visited = 0usize;

        while let Some(node) = queue.pop_front() {
            visited += 1;
            if let Some(succs) = successors.get(&node) {
                for s in succs {
                    if let Some(deg) = in_degree.get_mut(s) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push_back(s.clone());
                        }
                    }
                }
            }
        }

        if visited < ep.nodes.len() {
            errors.push(ValidationError {
                location: format!("endpoints.{}", ep_name),
                message: "Endpoint DAG contains a cycle.".to_string(),
                suggestion: None,
            });
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal valid ozzy.toml.
    const MINIMAL_TOML: &str = r#"
[project]
name = "test-project"
owner = "testuser"

[environments.default]
base = "ozzydb/python:3.12"
lockfile = "uv.lock"

[transforms.clean]
source = "transforms/clean.py:clean_fn"
environment = "default"
inputs.readings = "parquet"
output = "parquet"

[endpoints.cleaned]
description = "Cleaned data"

[endpoints.cleaned.nodes]
clean = { transform = "clean" }

[[endpoints.cleaned.edges]]
from = "data:raw"
to = "clean.readings"
"#;

    #[test]
    fn parse_minimal_toml() {
        let doc = OzzyToml::parse(MINIMAL_TOML).unwrap();
        assert_eq!(doc.project.name, "test-project");
        assert_eq!(doc.project.owner, "testuser");
        assert_eq!(doc.environments.len(), 1);
        assert_eq!(doc.transforms.len(), 1);
        assert_eq!(doc.endpoints.len(), 1);

        let errors = doc.validate();
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn parse_full_toml() {
        let toml = r#"
[project]
name = "sapflux-analysis"
owner = "rileyleff"
description = "Sap flux processing"

[git]
provider = "github"
repo = "rileyleff/sapflux-analysis"

[remote]
url = "https://api.ozzydb.com"

[environments.scipy-stack]
base = "ozzydb/python:3.12"
lockfile = "uv.lock"

[environments.geo]
dockerfile = "envs/geo.Dockerfile"

[environments.legacy]
image = "ghcr.io/user/image:v1"

[transforms.qc]
source = "transforms/qc.py:quality_control"
environment = "scipy-stack"
description = "Filter by voltage"
inputs.readings = "parquet"
inputs.metadata = "parquet"
output = "parquet"

[transforms.qc.params.threshold]
type = "float"
description = "Min voltage"
default = 11.5

[transforms.qc.params.method]
type = "string"
enum = ["voltage", "range"]

[transforms.cal]
source = "transforms/cal.py:apply_calibration"
environment = "scipy-stack"
inputs.data = "parquet"
inputs.constants = "parquet"
output = "parquet"

[transforms.cal.params.method]
type = "string"

[endpoints.corrected]
description = "QC'd and calibrated data"

[endpoints.corrected.params.qc_threshold]
type = "float"
default = 11.5
binds = "qc.threshold"
min = 0.0
max = 20.0

[endpoints.corrected.params.cal_method]
type = "string"
default = "leff_2024"
binds = "cal.method"
enum = ["leff_2024", "smith_2023"]

[endpoints.corrected.nodes]
qc = { transform = "qc", params = { method = "voltage" } }
cal = { transform = "cal" }

[[endpoints.corrected.edges]]
from = "data:raw_readings"
to = "qc.readings"

[[endpoints.corrected.edges]]
from = "data:site_metadata"
to = "qc.metadata"

[[endpoints.corrected.edges]]
from = "qc"
to = "cal.data"

[[endpoints.corrected.edges]]
from = "endpoint:vcr-lter/shared/constants@v1.0"
to = "cal.constants"
"#;

        let doc = OzzyToml::parse(toml).unwrap();
        assert_eq!(doc.environments.len(), 3);
        assert_eq!(doc.transforms.len(), 2);
        assert_eq!(doc.endpoints.len(), 1);

        let ep = &doc.endpoints["corrected"];
        assert_eq!(ep.nodes.len(), 2);
        assert_eq!(ep.edges.len(), 4);
        assert_eq!(ep.params.len(), 2);

        let errors = doc.validate();
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn validate_invalid_name() {
        let toml = r#"
[project]
name = "bad.name"
owner = "ok-owner"
"#;
        let doc = OzzyToml::parse(toml).unwrap();
        let errors = doc.validate();
        assert!(
            errors.iter().any(|e| e.location == "project.name"),
            "Expected error on project.name"
        );
    }

    #[test]
    fn validate_environment_tier_exclusivity() {
        let toml = r#"
[project]
name = "test"
owner = "user"

[environments.bad]
base = "ozzydb/python:3.12"
lockfile = "uv.lock"
image = "ghcr.io/bad:v1"
"#;
        let doc = OzzyToml::parse(toml).unwrap();
        let errors = doc.validate();
        assert!(
            errors.iter().any(|e| e.location == "environments.bad"),
            "Expected tier exclusivity error"
        );
    }

    #[test]
    fn validate_transform_source_xor_command() {
        let toml = r#"
[project]
name = "test"
owner = "user"

[environments.default]
base = "ozzydb/python:3.12"
lockfile = "uv.lock"

[transforms.both]
source = "transforms/t.py:fn"
command = "echo hello"
environment = "default"

[transforms.neither]
environment = "default"
"#;
        let doc = OzzyToml::parse(toml).unwrap();
        let errors = doc.validate();
        assert!(
            errors.iter().any(|e| e.location == "transforms.both"
                && e.message.contains("not both")),
            "Expected 'not both' error"
        );
        assert!(
            errors.iter().any(|e| e.location == "transforms.neither"
                && e.message.contains("either")),
            "Expected 'either' error"
        );
    }

    #[test]
    fn validate_environment_ref_missing() {
        let toml = r#"
[project]
name = "test"
owner = "user"

[environments.real-env]
base = "ozzydb/python:3.12"
lockfile = "uv.lock"

[transforms.t]
source = "t.py:fn"
environment = "nonexistent"
"#;
        let doc = OzzyToml::parse(toml).unwrap();
        let errors = doc.validate();
        let env_err = errors
            .iter()
            .find(|e| e.location == "transforms.t.environment")
            .expect("Expected environment ref error");
        assert!(env_err.message.contains("not found"));
        // Should suggest "real-env" based on edit distance
        assert_eq!(env_err.suggestion, None); // "nonexistent" is too far from "real-env"
    }

    #[test]
    fn validate_environment_ref_with_suggestion() {
        let toml = r#"
[project]
name = "test"
owner = "user"

[environments.scipy-stack]
base = "ozzydb/python:3.12"
lockfile = "uv.lock"

[transforms.t]
source = "t.py:fn"
environment = "scipy-stac"
"#;
        let doc = OzzyToml::parse(toml).unwrap();
        let errors = doc.validate();
        let env_err = errors
            .iter()
            .find(|e| e.location == "transforms.t.environment")
            .expect("Expected environment ref error");
        assert_eq!(env_err.suggestion.as_deref(), Some("scipy-stack"));
    }

    #[test]
    fn validate_node_transform_ref() {
        let toml = r#"
[project]
name = "test"
owner = "user"

[environments.default]
base = "ozzydb/python:3.12"
lockfile = "uv.lock"

[transforms.real_transform]
source = "t.py:fn"
environment = "default"
inputs.data = "parquet"

[endpoints.ep]

[endpoints.ep.nodes]
n = { transform = "nonexistent" }

[[endpoints.ep.edges]]
from = "data:raw"
to = "n.data"
"#;
        let doc = OzzyToml::parse(toml).unwrap();
        let errors = doc.validate();
        assert!(
            errors
                .iter()
                .any(|e| e.location.contains("nodes.n.transform")),
            "Expected transform ref error"
        );
    }

    #[test]
    fn validate_edge_target_invalid() {
        let toml = r#"
[project]
name = "test"
owner = "user"

[environments.default]
base = "ozzydb/python:3.12"
lockfile = "uv.lock"

[transforms.t]
source = "t.py:fn"
environment = "default"
inputs.data = "parquet"

[endpoints.ep]

[endpoints.ep.nodes]
n = { transform = "t" }

[[endpoints.ep.edges]]
from = "data:raw"
to = "just_node_no_input"
"#;
        let doc = OzzyToml::parse(toml).unwrap();
        let errors = doc.validate();
        assert!(
            errors.iter().any(|e| e.message.contains("node_name.input_name")),
            "Expected edge target format error"
        );
    }

    #[test]
    fn validate_input_coverage_missing() {
        let toml = r#"
[project]
name = "test"
owner = "user"

[environments.default]
base = "ozzydb/python:3.12"
lockfile = "uv.lock"

[transforms.t]
source = "t.py:fn"
environment = "default"
inputs.data = "parquet"
inputs.meta = "parquet"

[endpoints.ep]

[endpoints.ep.nodes]
n = { transform = "t" }

[[endpoints.ep.edges]]
from = "data:raw"
to = "n.data"
"#;
        let doc = OzzyToml::parse(toml).unwrap();
        let errors = doc.validate();
        assert!(
            errors.iter().any(|e| e.message.contains("meta")
                && e.message.contains("no incoming edge")),
            "Expected missing input error for 'meta', got: {:?}", errors
        );
    }

    #[test]
    fn validate_input_coverage_duplicate() {
        let toml = r#"
[project]
name = "test"
owner = "user"

[environments.default]
base = "ozzydb/python:3.12"
lockfile = "uv.lock"

[transforms.t]
source = "t.py:fn"
environment = "default"
inputs.data = "parquet"

[endpoints.ep]

[endpoints.ep.nodes]
n = { transform = "t" }

[[endpoints.ep.edges]]
from = "data:raw1"
to = "n.data"

[[endpoints.ep.edges]]
from = "data:raw2"
to = "n.data"
"#;
        let doc = OzzyToml::parse(toml).unwrap();
        let errors = doc.validate();
        assert!(
            errors.iter().any(|e| e.message.contains("2 incoming edges")),
            "Expected duplicate input error, got: {:?}", errors
        );
    }

    #[test]
    fn validate_cycle_detection() {
        let toml = r#"
[project]
name = "test"
owner = "user"

[environments.default]
base = "ozzydb/python:3.12"
lockfile = "uv.lock"

[transforms.t]
source = "t.py:fn"
environment = "default"
inputs.data = "parquet"

[endpoints.ep]

[endpoints.ep.nodes]
a = { transform = "t" }
b = { transform = "t" }

[[endpoints.ep.edges]]
from = "a"
to = "b.data"

[[endpoints.ep.edges]]
from = "b"
to = "a.data"
"#;
        let doc = OzzyToml::parse(toml).unwrap();
        let errors = doc.validate();
        assert!(
            errors.iter().any(|e| e.message.contains("cycle")),
            "Expected cycle error, got: {:?}", errors
        );
    }

    #[test]
    fn validate_param_binds_valid() {
        let toml = r#"
[project]
name = "test"
owner = "user"

[environments.default]
base = "ozzydb/python:3.12"
lockfile = "uv.lock"

[transforms.t]
source = "t.py:fn"
environment = "default"
inputs.data = "parquet"

[transforms.t.params.threshold]
type = "float"

[endpoints.ep]

[endpoints.ep.params.user_threshold]
type = "float"
default = 10.0
binds = "n.threshold"

[endpoints.ep.nodes]
n = { transform = "t" }

[[endpoints.ep.edges]]
from = "data:raw"
to = "n.data"
"#;
        let doc = OzzyToml::parse(toml).unwrap();
        let errors = doc.validate();
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn validate_param_binds_invalid_node() {
        let toml = r#"
[project]
name = "test"
owner = "user"

[environments.default]
base = "ozzydb/python:3.12"
lockfile = "uv.lock"

[transforms.t]
source = "t.py:fn"
environment = "default"
inputs.data = "parquet"

[endpoints.ep]

[endpoints.ep.params.user_threshold]
type = "float"
binds = "nonexistent.threshold"

[endpoints.ep.nodes]
n = { transform = "t" }

[[endpoints.ep.edges]]
from = "data:raw"
to = "n.data"
"#;
        let doc = OzzyToml::parse(toml).unwrap();
        let errors = doc.validate();
        assert!(
            errors.iter().any(|e| e.message.contains("nonexistent")),
            "Expected bind node error"
        );
    }

    #[test]
    fn validate_cross_project_endpoint_ref_unpinned() {
        let toml = r#"
[project]
name = "test"
owner = "user"

[environments.default]
base = "ozzydb/python:3.12"
lockfile = "uv.lock"

[transforms.t]
source = "t.py:fn"
environment = "default"
inputs.data = "parquet"

[endpoints.ep]

[endpoints.ep.nodes]
n = { transform = "t" }

[[endpoints.ep.edges]]
from = "endpoint:other-org/other-project/constants"
to = "n.data"
"#;
        let doc = OzzyToml::parse(toml).unwrap();
        let errors = doc.validate();
        assert!(
            errors.iter().any(|e| e.message.contains("@sha_or_tag")),
            "Expected cross-project pinning error, got: {:?}", errors
        );
    }

    #[test]
    fn validate_cross_project_endpoint_ref_pinned_ok() {
        let toml = r#"
[project]
name = "test"
owner = "user"

[environments.default]
base = "ozzydb/python:3.12"
lockfile = "uv.lock"

[transforms.t]
source = "t.py:fn"
environment = "default"
inputs.data = "parquet"

[endpoints.ep]

[endpoints.ep.nodes]
n = { transform = "t" }

[[endpoints.ep.edges]]
from = "endpoint:other-org/other-project/constants@v1.0"
to = "n.data"
"#;
        let doc = OzzyToml::parse(toml).unwrap();
        let errors = doc.validate();
        // Should not have the cross-project pinning error
        assert!(
            !errors.iter().any(|e| e.message.contains("@sha_or_tag")),
            "Should not have pinning error for pinned ref"
        );
    }

    #[test]
    fn validate_param_type_invalid() {
        let toml = r#"
[project]
name = "test"
owner = "user"

[environments.default]
base = "ozzydb/python:3.12"
lockfile = "uv.lock"

[transforms.t]
source = "t.py:fn"
environment = "default"
inputs.data = "parquet"

[transforms.t.params.bad_param]
type = "vector"
"#;
        let doc = OzzyToml::parse(toml).unwrap();
        let errors = doc.validate();
        assert!(
            errors.iter().any(|e| e.message.contains("vector")
                && e.message.contains("Must be")),
            "Expected invalid param type error, got: {:?}", errors
        );
    }

    #[test]
    fn parse_command_based_transform() {
        let toml = r#"
[project]
name = "test"
owner = "user"

[environments.geo]
dockerfile = "Dockerfile.geo"

[transforms.reproject]
command = "ogr2ogr -f GeoJSON ${output} ${input.boundaries}"
environment = "geo"
inputs.boundaries = "application/geo+json"
output = "application/geo+json"

[transforms.reproject.params.epsg]
type = "int"
"#;
        let doc = OzzyToml::parse(toml).unwrap();
        let t = &doc.transforms["reproject"];
        assert!(t.source.is_none());
        assert!(t.command.is_some());
        assert_eq!(t.inputs.len(), 1);

        let errors = doc.validate();
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn parse_network_and_secrets_transform() {
        let toml = r#"
[project]
name = "test"
owner = "user"

[environments.ml]
base = "ozzydb/python:3.12"
lockfile = "uv.lock"

[transforms.extract]
source = "transforms/extract.py:extract_fn"
environment = "ml"
network = true
secrets = ["GEMINI_API_KEY"]
inputs.pdf = "application/pdf"
output = "parquet"
"#;
        let doc = OzzyToml::parse(toml).unwrap();
        let t = &doc.transforms["extract"];
        assert!(t.network);
        assert_eq!(t.secrets, vec!["GEMINI_API_KEY"]);

        let errors = doc.validate();
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn parse_collection_types() {
        let toml = r#"
[project]
name = "test"
owner = "user"

[environments.default]
base = "ozzydb/python:3.12"
lockfile = "uv.lock"

[transforms.aggregate]
source = "transforms/agg.py:aggregate"
environment = "default"
inputs.data = "collection<parquet>"
output = "parquet"
"#;
        let doc = OzzyToml::parse(toml).unwrap();
        let t = &doc.transforms["aggregate"];
        assert_eq!(t.inputs["data"], "collection<parquet>");
        assert_eq!(t.output, "parquet");
    }

    #[test]
    fn parse_output_schema() {
        let toml = r#"
[project]
name = "test"
owner = "user"

[environments.default]
base = "ozzydb/python:3.12"
lockfile = "uv.lock"

[transforms.t]
source = "t.py:fn"
environment = "default"
inputs.data = "parquet"
output = "parquet"

[[transforms.t.output_schema.columns]]
name = "timestamp"
type = "timestamp[us]"

[[transforms.t.output_schema.columns]]
name = "flux"
type = "float64"
unit = "g/m2/s"

[transforms.t.output_schema.columns.constraints]
min = 0.0
"#;
        let doc = OzzyToml::parse(toml).unwrap();
        let schema = doc.transforms["t"].output_schema.as_ref().unwrap();
        assert_eq!(schema.columns.len(), 2);
        assert_eq!(schema.columns[0].name, "timestamp");
        assert_eq!(schema.columns[1].unit.as_deref(), Some("g/m2/s"));
    }

    #[test]
    fn parse_machine_config() {
        let toml = r#"
[project]
name = "test"
owner = "user"

[environments.default]
base = "ozzydb/python:3.12"
lockfile = "uv.lock"

[transforms.t]
source = "t.py:fn"
environment = "default"
inputs.data = "parquet"
output = "parquet"

[endpoints.ep]

[endpoints.ep.nodes]
heavy = { transform = "t", machine = "gpu-large" }
light = { transform = "t" }

[[endpoints.ep.edges]]
from = "data:raw"
to = "heavy.data"

[[endpoints.ep.edges]]
from = "heavy"
to = "light.data"
"#;
        let doc = OzzyToml::parse(toml).unwrap();
        let ep = &doc.endpoints["ep"];
        assert_eq!(ep.nodes["heavy"].machine.as_deref(), Some("gpu-large"));
        assert_eq!(ep.nodes["light"].machine, None);

        let errors = doc.validate();
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn validate_multiple_errors_at_once() {
        let toml = r#"
[project]
name = "bad.name"
owner = "also bad!"

[environments.default]
base = "ozzydb/python:3.12"
lockfile = "uv.lock"

[transforms.t]
source = "t.py:fn"
command = "echo hi"
environment = "missing-env"
inputs.data = "parquet"
"#;
        let doc = OzzyToml::parse(toml).unwrap();
        let errors = doc.validate();
        // Should have: bad project name, bad owner name, both source+command, missing env
        assert!(errors.len() >= 4, "Expected at least 4 errors, got {}: {:?}", errors.len(), errors);
    }

    #[test]
    fn edge_source_parsing() {
        assert_eq!(
            parse_edge_source("data:raw"),
            EdgeSource::Data("raw".to_string())
        );
        assert_eq!(
            parse_edge_source("collection:all"),
            EdgeSource::Collection("all".to_string())
        );
        assert_eq!(
            parse_edge_source("endpoint:org/proj/ep@v1"),
            EdgeSource::Endpoint("org/proj/ep@v1".to_string())
        );
        assert_eq!(
            parse_edge_source("qc"),
            EdgeSource::Node("qc".to_string())
        );
    }

    #[test]
    fn edge_target_parsing() {
        assert_eq!(
            parse_edge_target("clean.data"),
            Some(("clean".to_string(), "data".to_string()))
        );
        assert_eq!(parse_edge_target("noperiod"), None);
        assert_eq!(parse_edge_target(".data"), None);
        assert_eq!(parse_edge_target("clean."), None);
    }

    #[test]
    fn edit_distance_sanity() {
        assert_eq!(edit_distance("kitten", "sitting"), 3);
        assert_eq!(edit_distance("abc", "abc"), 0);
        assert_eq!(edit_distance("", "abc"), 3);
    }

    #[test]
    fn roundtrip_serialize() {
        let doc = OzzyToml::parse(MINIMAL_TOML).unwrap();
        let serialized = doc.to_toml_string().unwrap();
        let doc2 = OzzyToml::parse(&serialized).unwrap();
        assert_eq!(doc2.project.name, "test-project");
        assert_eq!(doc2.environments.len(), 1);
    }
}
