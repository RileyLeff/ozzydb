use anyhow::Result;
use ozzy_core::project::{Endpoint, PipelineEdge, PipelineNode, SourceType};
use ozzy_core::{commit, Project};
use std::fs;

pub async fn create(name: &str, input: &str, transforms: &[String]) -> Result<()> {
    let mut project = Project::find_current()?;

    // Verify input data source exists
    let data_sources = commit::collect_data_sources(&project)?;
    if !data_sources.contains_key(input) {
        anyhow::bail!(
            "Data source '{}' not found. Available: {}",
            input,
            data_sources.keys().cloned().collect::<Vec<_>>().join(", ")
        );
    }

    // Verify transforms exist
    let available_transforms = commit::collect_transforms(&project)?;
    for t in transforms {
        if !available_transforms.contains_key(t) {
            anyhow::bail!(
                "Transform '{}' not found. Available: {}",
                t,
                available_transforms.keys().cloned().collect::<Vec<_>>().join(", ")
            );
        }
    }

    // Build the pipeline nodes and edges
    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    let mut prev_source = input.to_string();
    let mut prev_source_type = SourceType::DataSource;

    for (i, transform_name) in transforms.iter().enumerate() {
        let node_name = if transforms.len() == 1 {
            name.to_string()
        } else {
            format!("{}_{}", name, i)
        };

        nodes.push(PipelineNode {
            node_name: node_name.clone(),
            transform_name: transform_name.clone(),
            params: serde_json::json!({}),
        });

        edges.push(PipelineEdge {
            target_node: node_name.clone(),
            input_name: "main".to_string(),
            source_type: prev_source_type.clone(),
            source_ref: prev_source.clone(),
            external_owner: None,
            external_project: None,
            external_endpoint: None,
            external_commit_hash: None,
        });

        prev_source = node_name;
        prev_source_type = SourceType::Node;
    }

    let endpoint = Endpoint {
        name: name.to_string(),
        nodes,
        edges,
        description: None,
    };

    // Save endpoint to a staging file (will be committed later)
    // For now, save to .ozzy/staged_endpoints/
    let staged_dir = project.ozzy_dir().join("staged_endpoints");
    fs::create_dir_all(&staged_dir)?;

    let endpoint_path = staged_dir.join(format!("{}.json", name));
    let content = serde_json::to_string_pretty(&endpoint)?;
    fs::write(&endpoint_path, content)?;

    // Also update workspace config
    if !project.config.workspace.staged_transforms.contains(&format!("endpoints/{}.json", name)) {
        project.config.workspace.staged_transforms.push(format!("endpoints/{}.json", name));
        project.save_config()?;
    }

    println!("Created endpoint: {}", name);
    println!();
    println!("Pipeline:");
    println!("  {} (data source)", input);
    for t in transforms {
        println!("    ↓");
        println!("  {} (transform)", t);
    }
    println!("    ↓");
    println!("  [output]");
    println!();
    println!("Run with: ozzy run {}", name);

    Ok(())
}

pub async fn list() -> Result<()> {
    let project = Project::find_current()?;

    // Check staged endpoints
    let staged_dir = project.ozzy_dir().join("staged_endpoints");
    let mut endpoints = Vec::new();

    if staged_dir.exists() {
        for entry in fs::read_dir(&staged_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map(|e| e == "json").unwrap_or(false) {
                let content = fs::read_to_string(&path)?;
                let endpoint: Endpoint = serde_json::from_str(&content)?;
                endpoints.push((endpoint, false)); // false = not committed
            }
        }
    }

    // Check committed endpoints
    if let Some(commit) = project.latest_commit()? {
        for (_, endpoint) in commit.endpoints {
            endpoints.push((endpoint, true)); // true = committed
        }
    }

    if endpoints.is_empty() {
        println!("No endpoints found.");
        println!();
        println!("Create an endpoint with:");
        println!("  ozzy endpoint create <name> --input <data> --transforms <t1,t2,...>");
        return Ok(());
    }

    println!("Endpoints:");
    for (endpoint, committed) in &endpoints {
        let status = if *committed { "" } else { " (staged)" };
        let transforms: Vec<_> = endpoint.nodes.iter().map(|n| n.transform_name.as_str()).collect();
        println!("  {}{}", endpoint.name, status);
        println!("    Transforms: {}", transforms.join(" → "));
    }

    Ok(())
}

pub async fn remove(name: &str) -> Result<()> {
    let project = Project::find_current()?;

    // Check staged endpoints first
    let staged_path = project.ozzy_dir().join("staged_endpoints").join(format!("{}.json", name));
    if staged_path.exists() {
        fs::remove_file(&staged_path)?;
        println!("Removed staged endpoint: {}", name);
        return Ok(());
    }

    // Check if it's a committed endpoint
    if let Some(commit) = project.latest_commit()? {
        if commit.endpoints.contains_key(name) {
            anyhow::bail!(
                "Endpoint '{}' is committed. Create a new commit without this endpoint to remove it.",
                name
            );
        }
    }

    anyhow::bail!("Endpoint '{}' not found", name);
}

pub async fn show(name: &str) -> Result<()> {
    let project = Project::find_current()?;

    // Check staged endpoints first
    let staged_path = project.ozzy_dir().join("staged_endpoints").join(format!("{}.json", name));
    if staged_path.exists() {
        let content = fs::read_to_string(&staged_path)?;
        let endpoint: Endpoint = serde_json::from_str(&content)?;
        print_endpoint(&endpoint, false);
        return Ok(());
    }

    // Check committed endpoints
    if let Some(commit) = project.latest_commit()? {
        if let Some(endpoint) = commit.endpoints.get(name) {
            print_endpoint(endpoint, true);
            return Ok(());
        }
    }

    anyhow::bail!("Endpoint '{}' not found", name);
}

fn print_endpoint(endpoint: &Endpoint, committed: bool) {
    let status = if committed { "committed" } else { "staged" };
    println!("Endpoint: {} ({})", endpoint.name, status);

    if let Some(desc) = &endpoint.description {
        println!("Description: {}", desc);
    }

    println!();
    println!("Pipeline:");
    for edge in &endpoint.edges {
        let source = match edge.source_type {
            SourceType::DataSource => format!("{} (data)", edge.source_ref),
            SourceType::Node => format!("{} (node)", edge.source_ref),
            SourceType::External => format!(
                "{}/{}/{}@{} (external)",
                edge.external_owner.as_deref().unwrap_or("?"),
                edge.external_project.as_deref().unwrap_or("?"),
                edge.external_endpoint.as_deref().unwrap_or("?"),
                edge.external_commit_hash.as_deref().unwrap_or("?")
            ),
        };
        println!("  {} -> {} [input: {}]", source, edge.target_node, edge.input_name);
    }

    println!();
    println!("Nodes:");
    for node in &endpoint.nodes {
        println!("  {}: {}", node.node_name, node.transform_name);
        if !node.params.is_null() && node.params != serde_json::json!({}) {
            println!("    params: {}", node.params);
        }
    }
}
