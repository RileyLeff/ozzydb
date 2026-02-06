use anyhow::Result;
use ozzy_core::project::{Endpoint, SourceType};
use ozzy_core::{Project, commit};
use std::fs;

pub async fn show(format: &str, endpoint_name: Option<&str>) -> Result<()> {
    let project = Project::find_current()?;

    let endpoints = collect_all_endpoints(&project)?;

    if endpoints.is_empty() {
        println!("No endpoints defined.");
        return Ok(());
    }

    let endpoints_to_show: Vec<_> = if let Some(name) = endpoint_name {
        endpoints.into_iter().filter(|e| e.name == name).collect()
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
        "svg" => print_svg_dag(&project, &endpoints_to_show)?,
        _ => anyhow::bail!(
            "Unknown format: {}. Supported: ascii, svg, json, mermaid",
            format
        ),
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

            let node = endpoint
                .nodes
                .iter()
                .find(|n| n.node_name == edge.target_node);
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
        println!(
            "    subgraph {}[\"Endpoint: {}\"]",
            endpoint.name, endpoint.name
        );

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

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn print_svg_dag(project: &Project, endpoints: &[Endpoint]) -> Result<()> {
    let data_sources = commit::collect_data_sources(project)?;
    let transforms = commit::collect_transforms(project)?;

    let mut total_height = 40usize;
    for endpoint in endpoints {
        let rows = endpoint.edges.len().max(1);
        total_height += 60 + rows * 90 + 40;
    }

    let width = 920usize;
    println!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">",
        width, total_height, width, total_height
    );
    println!("<defs>");
    println!(
        "<marker id=\"arrow\" markerWidth=\"10\" markerHeight=\"7\" refX=\"9\" refY=\"3.5\" orient=\"auto\">"
    );
    println!("<polygon points=\"0 0, 10 3.5, 0 7\" fill=\"#334155\" />");
    println!("</marker>");
    println!("</defs>");
    println!("<rect width=\"100%\" height=\"100%\" fill=\"#f8fafc\" />");

    let mut y_offset = 30i32;
    for endpoint in endpoints {
        let rows = endpoint.edges.len().max(1) as i32;
        let block_height = 60 + rows * 90;
        println!(
            "<rect x=\"16\" y=\"{}\" width=\"888\" height=\"{}\" rx=\"10\" fill=\"#ffffff\" stroke=\"#cbd5e1\" />",
            y_offset - 18,
            block_height + 22
        );
        println!(
            "<text x=\"30\" y=\"{}\" font-family=\"monospace\" font-size=\"16\" fill=\"#0f172a\">Endpoint: {}</text>",
            y_offset + 6,
            escape_xml(&endpoint.name)
        );

        let mut row = 0i32;
        for edge in &endpoint.edges {
            let y = y_offset + 28 + row * 90;
            row += 1;

            let source_label = match edge.source_type {
                SourceType::DataSource => data_sources
                    .get(&edge.source_ref)
                    .map(|ds| ds.name.clone())
                    .unwrap_or_else(|| edge.source_ref.clone()),
                SourceType::Node => edge.source_ref.clone(),
                SourceType::External => format!(
                    "{}/{}/{}",
                    edge.external_owner.as_deref().unwrap_or("?"),
                    edge.external_project.as_deref().unwrap_or("?"),
                    edge.external_endpoint.as_deref().unwrap_or("?")
                ),
            };

            let target_label = endpoint
                .nodes
                .iter()
                .find(|n| n.node_name == edge.target_node)
                .map(|n| {
                    let runtime_suffix = transforms
                        .get(&n.transform_name)
                        .map(|t| format!(" [{}]", t.runtime))
                        .unwrap_or_default();
                    format!("{}{}", n.transform_name, runtime_suffix)
                })
                .unwrap_or_else(|| edge.target_node.clone());

            println!(
                "<rect x=\"40\" y=\"{}\" width=\"260\" height=\"44\" rx=\"6\" fill=\"#e2e8f0\" stroke=\"#94a3b8\" />",
                y - 22
            );
            println!(
                "<text x=\"52\" y=\"{}\" font-family=\"monospace\" font-size=\"13\" fill=\"#0f172a\">{}</text>",
                y + 4,
                escape_xml(&source_label)
            );

            println!(
                "<rect x=\"400\" y=\"{}\" width=\"430\" height=\"44\" rx=\"6\" fill=\"#dbeafe\" stroke=\"#60a5fa\" />",
                y - 22
            );
            println!(
                "<text x=\"412\" y=\"{}\" font-family=\"monospace\" font-size=\"13\" fill=\"#0f172a\">{}</text>",
                y + 4,
                escape_xml(&target_label)
            );

            println!(
                "<line x1=\"300\" y1=\"{}\" x2=\"400\" y2=\"{}\" stroke=\"#334155\" stroke-width=\"2\" marker-end=\"url(#arrow)\" />",
                y, y
            );
        }

        y_offset += block_height + 46;
    }

    println!("</svg>");
    Ok(())
}
