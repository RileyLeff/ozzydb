//! `ozzy endpoint` — inspect endpoints defined in ozzy.toml.

use anyhow::{Result, bail};
use serde::Deserialize;
use std::collections::HashMap;

use super::shared;

// ── Response types ──────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct EndpointSummary {
    name: String,
    description: Option<String>,
    params: Vec<ParamSummary>,
    node_count: usize,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct ParamSummary {
    name: String,
    #[serde(rename = "type")]
    type_: String,
    description: Option<String>,
    default: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct EndpointDetail {
    name: String,
    description: Option<String>,
    commit_sha: String,
    params: Vec<ParamDetail>,
    nodes: HashMap<String, NodeDetail>,
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
    transform: String,
    #[allow(dead_code)]
    params: Option<HashMap<String, serde_json::Value>>,
    machine: Option<String>,
}

#[derive(Debug, Deserialize)]
struct EdgeDetail {
    from: String,
    to: String,
}

#[derive(Debug, Deserialize)]
struct DagResponse {
    #[allow(dead_code)]
    format: String,
    content: String,
}

// ── Commands ────────────────────────────────────────────────────

/// List endpoints in the current project.
pub async fn ls(ref_name: Option<&str>) -> Result<()> {
    let creds = shared::require_auth()?;
    let project = shared::load_project_from_toml()?;
    let registry_url = shared::registry_url(&creds);
    let client = shared::http_client()?;

    let url = format!(
        "{}/api/v1/endpoints/{}/{}",
        registry_url, project.owner, project.slug
    );

    let mut request = client.get(&url).bearer_auth(&creds.token);
    if let Some(r) = ref_name {
        request = request.query(&[("ref", r)]);
    }

    let resp = request.send().await?;

    if !resp.status().is_success() {
        let err = shared::extract_error(resp).await;
        bail!("Failed to list endpoints: {}", err);
    }

    let endpoints: Vec<EndpointSummary> = resp.json().await?;

    if endpoints.is_empty() {
        println!("No endpoints defined. Add endpoints in ozzy.toml.");
        return Ok(());
    }

    println!(
        "{:<24} {:<6} {:<8} {}",
        "NAME", "NODES", "PARAMS", "DESCRIPTION"
    );
    for ep in &endpoints {
        println!(
            "{:<24} {:<6} {:<8} {}",
            ep.name,
            ep.node_count,
            ep.params.len(),
            ep.description.as_deref().unwrap_or(""),
        );
    }

    Ok(())
}

/// Show endpoint details.
pub async fn show(name: &str, ref_name: Option<&str>) -> Result<()> {
    shared::validate_name(name, "endpoint")?;
    let creds = shared::require_auth()?;
    let project = shared::load_project_from_toml()?;
    let registry_url = shared::registry_url(&creds);
    let client = shared::http_client()?;

    let url = format!(
        "{}/api/v1/endpoints/{}/{}/{}",
        registry_url, project.owner, project.slug, name
    );

    let mut request = client.get(&url).bearer_auth(&creds.token);
    if let Some(r) = ref_name {
        request = request.query(&[("ref", r)]);
    }

    let resp = request.send().await?;

    if !resp.status().is_success() {
        let err = shared::extract_error(resp).await;
        bail!("Failed to get endpoint '{}': {}", name, err);
    }

    let detail: EndpointDetail = resp.json().await?;

    println!("Endpoint: {}", detail.name);
    if let Some(desc) = &detail.description {
        println!("Description: {}", desc);
    }
    println!(
        "Commit: {}",
        detail.commit_sha.get(..8).unwrap_or(&detail.commit_sha)
    );

    if !detail.params.is_empty() {
        println!("\nParameters:");
        for p in &detail.params {
            let default_str = p
                .default
                .as_ref()
                .map(|v| format!(" (default: {})", v))
                .unwrap_or_default();
            println!("  {} ({}){}", p.name, p.type_, default_str);
            if let Some(desc) = &p.description {
                println!("    {}", desc);
            }
            if p.min.is_some() || p.max.is_some() {
                let range = match (p.min, p.max) {
                    (Some(lo), Some(hi)) => format!("[{}, {}]", lo, hi),
                    (Some(lo), None) => format!("[{}, ...)", lo),
                    (None, Some(hi)) => format!("(..., {}]", hi),
                    (None, None) => unreachable!(),
                };
                println!("    range: {}", range);
            }
            if let Some(vals) = &p.enum_values {
                let strs: Vec<String> = vals.iter().map(|v| v.to_string()).collect();
                println!("    allowed: {}", strs.join(", "));
            }
            if !p.binds.is_empty() {
                println!("    binds to: {}", p.binds);
            }
        }
    }

    println!("\nNodes ({}):", detail.nodes.len());
    for (node_name, node) in &detail.nodes {
        let machine = node
            .machine
            .as_ref()
            .map(|m| format!(" [{}]", m))
            .unwrap_or_default();
        println!("  {} → {}{}", node_name, node.transform, machine);
    }

    if !detail.edges.is_empty() {
        println!("\nEdges:");
        for e in &detail.edges {
            println!("  {} → {}", e.from, e.to);
        }
    }

    Ok(())
}

/// Show endpoint DAG visualization.
pub async fn dag(name: &str, format: &str, ref_name: Option<&str>) -> Result<()> {
    shared::validate_name(name, "endpoint")?;
    let creds = shared::require_auth()?;
    let project = shared::load_project_from_toml()?;
    let registry_url = shared::registry_url(&creds);
    let client = shared::http_client()?;

    let url = format!(
        "{}/api/v1/endpoints/{}/{}/{}/dag",
        registry_url, project.owner, project.slug, name,
    );

    let mut query: Vec<(&str, &str)> = vec![("format", format)];
    if let Some(r) = ref_name {
        query.push(("ref", r));
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
    println!("{}", dag.content);

    Ok(())
}

/// Yank an endpoint version (not yet supported by server).
pub async fn yank(_name: &str, _ref_name: &str, _reason: &str) -> Result<()> {
    bail!("Endpoint yanking is not yet implemented. Yank individual data atoms instead.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_endpoint_summary_deserialization() {
        let json = serde_json::json!({
            "name": "corrected",
            "description": "Quality-controlled data",
            "params": [{
                "name": "threshold",
                "type": "float",
                "description": "QC threshold",
                "default": 0.5,
            }],
            "node_count": 2,
        });
        let ep: EndpointSummary = serde_json::from_value(json).unwrap();
        assert_eq!(ep.name, "corrected");
        assert_eq!(ep.node_count, 2);
        assert_eq!(ep.params.len(), 1);
    }

    #[test]
    fn test_dag_response_deserialization() {
        let json = serde_json::json!({
            "format": "mermaid",
            "content": "graph TD\n    A --> B",
        });
        let dag: DagResponse = serde_json::from_value(json).unwrap();
        assert_eq!(dag.format, "mermaid");
        assert!(dag.content.contains("graph TD"));
    }
}
