//! `ozzy endpoint` — inspect published v4 endpoints.

use anyhow::{Result, bail};
use serde::Deserialize;

use super::shared;

#[derive(Debug, Deserialize)]
struct EndpointSummary {
    name: String,
    description: Option<String>,
    inputs: Vec<TypedPortDetail>,
    params: Vec<serde_json::Value>,
    node_count: usize,
    edge_count: usize,
    terminal_node: String,
}

#[derive(Debug, Deserialize)]
struct EndpointDetail {
    name: String,
    description: Option<String>,
    commit_sha: String,
    project_revision_id: String,
    registry_revision_id: String,
    terminal_node: String,
    inputs: Vec<TypedPortDetail>,
    params: Vec<ParamDetail>,
    nodes: Vec<NodeDetail>,
    edges: Vec<EdgeDetail>,
}

#[derive(Debug, Deserialize)]
struct ParamDetail {
    name: String,
    #[serde(rename = "type")]
    type_: String,
    description: Option<String>,
    default: Option<serde_json::Value>,
    min: Option<f64>,
    max: Option<f64>,
    #[serde(rename = "enum")]
    enum_values: Option<Vec<serde_json::Value>>,
    binds: String,
}

#[derive(Debug, Deserialize)]
struct NodeDetail {
    name: String,
    params: std::collections::HashMap<String, serde_json::Value>,
    transform: TransformInspection,
}

#[derive(Debug, Deserialize)]
struct EdgeDetail {
    from: String,
    to: String,
}

#[derive(Debug, Deserialize)]
struct TypedPortDetail {
    name: String,
    description: Option<String>,
    #[serde(rename = "type")]
    ty: TypeRefDetail,
}

#[derive(Debug, Deserialize)]
struct TypeRefDetail {
    reference: String,
    canonical_type_key: String,
}

#[derive(Debug, Deserialize)]
struct TransformInspection {
    authored_name: String,
    versioned_name: String,
    transform_version_id: String,
    description: Option<String>,
    source: Option<String>,
    command: Option<String>,
    network: bool,
    secrets: Vec<String>,
    environment: TransformEnvironmentRef,
    inputs: Vec<TypedPortDetail>,
    outputs: Vec<TypedPortDetail>,
}

#[derive(Debug, Deserialize)]
struct TransformEnvironmentRef {
    versioned_name: String,
    environment_version_id: String,
}

#[derive(Debug, Deserialize)]
struct DagResponse {
    format: String,
    content: String,
}

pub async fn ls(ref_name: Option<&str>) -> Result<()> {
    let creds = shared::require_auth()?;
    let project = shared::load_project_from_toml()?;
    let client = shared::http_client()?;

    let url = format!(
        "{}/api/v1/endpoints/{}/{}",
        shared::registry_url(&creds),
        project.owner,
        project.slug
    );

    let mut request = client.get(&url).bearer_auth(&creds.token);
    if let Some(ref_name) = ref_name {
        request = request.query(&[("ref", ref_name)]);
    }

    let resp = request.send().await?;
    if !resp.status().is_success() {
        let err = shared::extract_error(resp).await;
        bail!("Failed to list endpoints: {}", err);
    }

    let endpoints: Vec<EndpointSummary> = resp.json().await?;
    if endpoints.is_empty() {
        println!("No endpoints defined.");
        return Ok(());
    }

    println!(
        "{:<24} {:<6} {:<6} {:<8} {:<8} {:<16} {}",
        "NAME", "NODES", "EDGES", "INPUTS", "PARAMS", "TERMINAL", "DESCRIPTION"
    );
    for endpoint in endpoints {
        println!(
            "{:<24} {:<6} {:<6} {:<8} {:<8} {:<16} {}",
            endpoint.name,
            endpoint.node_count,
            endpoint.edge_count,
            endpoint.inputs.len(),
            endpoint.params.len(),
            endpoint.terminal_node,
            endpoint.description.as_deref().unwrap_or("")
        );
    }

    Ok(())
}

pub async fn show(name: &str, ref_name: Option<&str>) -> Result<()> {
    shared::validate_name(name, "endpoint")?;
    let creds = shared::require_auth()?;
    let project = shared::load_project_from_toml()?;
    let client = shared::http_client()?;

    let url = format!(
        "{}/api/v1/endpoints/{}/{}/{}",
        shared::registry_url(&creds),
        project.owner,
        project.slug,
        name
    );

    let mut request = client.get(&url).bearer_auth(&creds.token);
    if let Some(ref_name) = ref_name {
        request = request.query(&[("ref", ref_name)]);
    }

    let resp = request.send().await?;
    if !resp.status().is_success() {
        let err = shared::extract_error(resp).await;
        bail!("Failed to get endpoint '{}': {}", name, err);
    }

    let detail: EndpointDetail = resp.json().await?;
    println!("Endpoint:           {}", detail.name);
    if let Some(description) = detail.description.as_deref() {
        println!("Description:        {}", description);
    }
    println!("Commit:             {}", detail.commit_sha);
    println!("Project revision:   {}", detail.project_revision_id);
    println!("Registry revision:  {}", detail.registry_revision_id);
    println!("Terminal node:      {}", detail.terminal_node);

    if !detail.inputs.is_empty() {
        println!("\nInputs:");
        for input in &detail.inputs {
            print_port(input, 2);
        }
    }

    if !detail.params.is_empty() {
        println!("\nParameters:");
        for param in &detail.params {
            let mut line = format!("  {} ({})", param.name, param.type_);
            if let Some(default) = &param.default {
                line.push_str(&format!(" default={}", default));
            }
            println!("{}", line);
            if let Some(description) = &param.description {
                println!("    {}", description);
            }
            if param.min.is_some() || param.max.is_some() {
                let range = match (param.min, param.max) {
                    (Some(lo), Some(hi)) => format!("[{}, {}]", lo, hi),
                    (Some(lo), None) => format!("[{}, ...)", lo),
                    (None, Some(hi)) => format!("(..., {}]", hi),
                    (None, None) => unreachable!(),
                };
                println!("    range={}", range);
            }
            if let Some(enum_values) = &param.enum_values {
                let joined = enum_values
                    .iter()
                    .map(|value| value.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                println!("    enum={}", joined);
            }
            if !param.binds.is_empty() {
                println!("    binds={}", param.binds);
            }
        }
    }

    println!("\nNodes:");
    for node in &detail.nodes {
        println!(
            "  {} -> {} ({})",
            node.name, node.transform.authored_name, node.transform.versioned_name
        );
        println!(
            "    transform_version_id={} environment={} ({}) network={}",
            node.transform.transform_version_id,
            node.transform.environment.versioned_name,
            node.transform.environment.environment_version_id,
            node.transform.network
        );
        if let Some(description) = &node.transform.description {
            println!("    description={}", description);
        }
        if let Some(source) = &node.transform.source {
            println!("    source={}", source);
        }
        if let Some(command) = &node.transform.command {
            println!("    command={}", command);
        }
        if !node.params.is_empty() {
            println!(
                "    node_params={}",
                serde_json::to_string_pretty(&node.params)?
            );
        }
        if !node.transform.secrets.is_empty() {
            println!("    secrets={}", node.transform.secrets.join(", "));
        }
        if !node.transform.inputs.is_empty() {
            println!("    inputs:");
            for input in &node.transform.inputs {
                print_port(input, 6);
            }
        }
        if !node.transform.outputs.is_empty() {
            println!("    outputs:");
            for output in &node.transform.outputs {
                print_port(output, 6);
            }
        }
    }

    if !detail.edges.is_empty() {
        println!("\nEdges:");
        for edge in &detail.edges {
            println!("  {} -> {}", edge.from, edge.to);
        }
    }

    Ok(())
}

pub async fn dag(name: &str, format: &str, ref_name: Option<&str>) -> Result<()> {
    shared::validate_name(name, "endpoint")?;
    let creds = shared::require_auth()?;
    let project = shared::load_project_from_toml()?;
    let client = shared::http_client()?;

    let url = format!(
        "{}/api/v1/endpoints/{}/{}/{}/dag",
        shared::registry_url(&creds),
        project.owner,
        project.slug,
        name
    );

    let mut query = vec![("format", format.to_string())];
    if let Some(ref_name) = ref_name {
        query.push(("ref", ref_name.to_string()));
    }

    let resp = client
        .get(&url)
        .bearer_auth(&creds.token)
        .query(&query)
        .send()
        .await?;

    if !resp.status().is_success() {
        let err = shared::extract_error(resp).await;
        bail!("Failed to get DAG for '{}': {}", name, err);
    }

    let dag: DagResponse = resp.json().await?;
    let _ = dag.format;
    println!("{}", dag.content);
    Ok(())
}

fn print_port(port: &TypedPortDetail, indent: usize) {
    let prefix = " ".repeat(indent);
    println!(
        "{}{}: {} [{}]",
        prefix, port.name, port.ty.reference, port.ty.canonical_type_key
    );
    if let Some(description) = &port.description {
        println!("{}  {}", prefix, description);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_summary_deserializes_v4_shape() {
        let json = serde_json::json!({
            "name": "corrected",
            "description": "Quality-controlled data",
            "inputs": [{
                "name": "raw",
                "description": null,
                "type": {
                    "reference": "RawCsv@1",
                    "canonical_type_key": "t:abc"
                }
            }],
            "params": [{
                "name": "threshold",
                "type": "float",
                "description": "QC threshold",
                "default": 0.5
            }],
            "node_count": 2,
            "edge_count": 1,
            "terminal_node": "sink"
        });
        let endpoint: EndpointSummary = serde_json::from_value(json).unwrap();
        assert_eq!(endpoint.name, "corrected");
        assert_eq!(endpoint.inputs.len(), 1);
        assert_eq!(endpoint.edge_count, 1);
        assert_eq!(endpoint.terminal_node, "sink");
    }
}
