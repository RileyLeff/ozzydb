use anyhow::Result;
use ozzy_core::project::{Endpoint, SourceType};
use ozzy_core::{commit, Project};
use std::fs;

pub async fn show(format: &str, endpoint_name: Option<&str>) -> Result<()> {
    let project = Project::find_current()?;

    let endpoints = collect_all_endpoints(&project)?;

    if endpoints.is_empty() {
        println!("No endpoints defined.");
        return Ok(());
    }

    let endpoints_to_show: Vec<_> = if let Some(name) = endpoint_name {
        endpoints
            .into_iter()
            .filter(|e| e.name == name)
            .collect()
    } else {
        endpoints
    };

    if endpoints_to_show.is_empty() {
        if let Some(name) = endpoint_name {
            anyhow::bail!("Endpoint '{}' not found", name);
        }
    }

    match format {
        "ascii" => print_ascii_dag(&project, &endpoints_to_show)?,
        "json" => print_json_dag(&endpoints_to_show)?,
        "mermaid" => print_mermaid_dag(&project, &endpoints_to_show)?,
        _ => anyhow::bail!("Unknown format: {}. Supported: ascii, json, mermaid", format),
    }

    Ok(())
}

fn collect_all_endpoints(project: &Project) -> Result<Vec<Endpoint>> {
    let mut endpoints = Vec::new();

    // Check staged endpoints
    let staged_dir = project.ozzy_dir().join("staged_endpoints");
    if staged_dir.exists() {
        for entry in fs::read_dir(&staged_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map(|e| e == "json").unwrap_or(false) {
                let content = fs::read_to_string(&path)?;
                let endpoint: Endpoint = serde_json::from_str(&content)?;
                endpoints.push(endpoint);
            }
        }
    }

    // Check committed endpoints
    if let Some(commit) = project.latest_commit()? {
        for (_, endpoint) in commit.endpoints {
            // Don't duplicate if already in staged
            if !endpoints.iter().any(|e| e.name == endpoint.name) {
                endpoints.push(endpoint);
            }
        }
    }

    Ok(endpoints)
}

fn print_ascii_dag(project: &Project, endpoints: &[Endpoint]) -> Result<()> {
    let data_sources = commit::collect_data_sources(project)?;
    let transforms = commit::collect_transforms(project)?;

    for endpoint in endpoints {
        println!("Endpoint: {}", endpoint.name);
        println!();

        // Build a simple ASCII representation
        for edge in &endpoint.edges {
            let source_name = match edge.source_type {
                SourceType::DataSource => {
                    if let Some(ds) = data_sources.get(&edge.source_ref) {
                        format!("[{}] (data, {} bytes)", ds.name, ds.byte_size.unwrap_or(0))
                    } else {
                        format!("[{}] (data, missing!)", edge.source_ref)
                    }
                }
                SourceType::Node => format!("({})", edge.source_ref),
                SourceType::External => format!(
                    "[{}/{}/{}] (external)",
                    edge.external_owner.as_deref().unwrap_or("?"),
                    edge.external_project.as_deref().unwrap_or("?"),
                    edge.external_endpoint.as_deref().unwrap_or("?")
                ),
            };

            let node = endpoint.nodes.iter().find(|n| n.node_name == edge.target_node);
            let transform_info = if let Some(n) = node {
                if let Some(t) = transforms.get(&n.transform_name) {
                    format!("{} [{}]", n.transform_name, t.runtime)
                } else {
                    format!("{} (missing!)", n.transform_name)
                }
            } else {
                "?".to_string()
            };

            println!("  {} ", source_name);
            println!("    │");
            println!("    ▼");
            println!("  ({}) ← {}", edge.target_node, transform_info);
        }

        println!("    │");
        println!("    ▼");
        println!("  [output]");
        println!();
    }

    Ok(())
}

fn print_json_dag(endpoints: &[Endpoint]) -> Result<()> {
    let json = serde_json::to_string_pretty(endpoints)?;
    println!("{}", json);
    Ok(())
}

fn print_mermaid_dag(project: &Project, endpoints: &[Endpoint]) -> Result<()> {
    let data_sources = commit::collect_data_sources(project)?;

    println!("```mermaid");
    println!("flowchart TD");

    for endpoint in endpoints {
        println!("    subgraph {}[\"Endpoint: {}\"]", endpoint.name, endpoint.name);

        // Add data source nodes
        for edge in &endpoint.edges {
            if edge.source_type == SourceType::DataSource {
                let ds_id = format!("ds_{}", edge.source_ref);
                if let Some(ds) = data_sources.get(&edge.source_ref) {
                    println!("        {}[(\"{}\")]", ds_id, ds.name);
                } else {
                    println!("        {}[(\"{}\")]", ds_id, edge.source_ref);
                }
            }
        }

        // Add transform nodes
        for node in &endpoint.nodes {
            let node_id = format!("node_{}", node.node_name);
            println!("        {}[{}]", node_id, node.transform_name);
        }

        // Add edges
        for edge in &endpoint.edges {
            let source_id = match edge.source_type {
                SourceType::DataSource => format!("ds_{}", edge.source_ref),
                SourceType::Node => format!("node_{}", edge.source_ref),
                SourceType::External => format!("ext_{}", edge.source_ref),
            };
            let target_id = format!("node_{}", edge.target_node);
            println!("        {} --> {}", source_id, target_id);
        }

        // Add output node
        if let Some(last_node) = endpoint.nodes.last() {
            let last_id = format!("node_{}", last_node.node_name);
            let output_id = format!("out_{}", endpoint.name);
            println!("        {}((output))", output_id);
            println!("        {} --> {}", last_id, output_id);
        }

        println!("    end");
    }

    println!("```");

    Ok(())
}
