use anyhow::Result;
use ozzy_core::project::{Endpoint, PipelineEdge, PipelineNode, SourceType};
use ozzy_core::{commit, schema, Project};
use std::fs;

pub async fn create(name: &str, input: &str, transforms: &[String]) -> Result<()> {
    let mut project = Project::find_current()?;

    // Verify input data source exists
    let data_sources = commit::collect_data_sources(&project)?;
    let data_source = data_sources.get(input).ok_or_else(|| {
        anyhow::anyhow!(
            "Data source '{}' not found. Available: {}",
            input,
            data_sources.keys().cloned().collect::<Vec<_>>().join(", ")
        )
    })?;

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

    // Validate schema compatibility
    let data_path = project.root.join(&data_source.path);
    let input_schema = schema::extract_parquet_schema(&data_path)?;

    println!("Validating pipeline schema...");
    println!();
    println!("Input schema ({}):", input);
    for field in &input_schema.fields {
        let nullable = if field.nullable { "?" } else { "" };
        println!("  {}: {}{}", field.name, field.dtype, nullable);
    }
    println!();

    // For now, we do basic validation - transforms must have access to all input columns
    // In a full implementation, we'd track schema transformations through each step
    let validation_result = validate_pipeline_schema(&input_schema, transforms, &available_transforms);

    if !validation_result.valid {
        println!("Schema validation failed:");
        for err in &validation_result.errors {
            println!("  ✗ {}", err);
        }
        anyhow::bail!("Pipeline validation failed. Fix schema issues before creating endpoint.");
    }

    for warning in &validation_result.warnings {
        println!("  ⚠ {}", warning);
    }

    println!("  ✓ Schema validation passed");
    println!();

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

/// Validate schema compatibility through the pipeline.
fn validate_pipeline_schema(
    input_schema: &schema::SchemaInfo,
    transforms: &[String],
    available_transforms: &std::collections::HashMap<String, ozzy_core::project::Transform>,
) -> schema::ValidationResult {
    let mut result = schema::ValidationResult::ok();

    // Track current schema (starts with input data source schema)
    let current_columns: Vec<&str> = input_schema.column_names();

    for (i, transform_name) in transforms.iter().enumerate() {
        let transform = match available_transforms.get(transform_name) {
            Some(t) => t,
            None => {
                result.valid = false;
                result.errors.push(format!("Step {}: Transform '{}' not found", i + 1, transform_name));
                continue;
            }
        };

        // Check if transform has input schema requirements
        if let Some(input_schema_value) = &transform.input_schema {
            if let Some(required) = input_schema_value.get("requires") {
                if let Some(required_cols) = required.as_array() {
                    for col in required_cols {
                        if let Some(col_name) = col.as_str() {
                            if !current_columns.contains(&col_name) {
                                result.valid = false;
                                result.errors.push(format!(
                                    "Step {}: Transform '{}' requires column '{}' which is not available. Available: {:?}",
                                    i + 1,
                                    transform_name,
                                    col_name,
                                    current_columns
                                ));
                            }
                        }
                    }
                }
            }
        }

        // Check if transform has output schema that adds columns
        if let Some(output_schema_value) = &transform.output_schema {
            if let Some(adds) = output_schema_value.get("adds") {
                if let Some(added_cols) = adds.as_array() {
                    for col in added_cols {
                        if let Some(col_name) = col.as_str() {
                            // In a real implementation, we'd add these to current_columns
                            // For now, just log that we know about them
                            result.warnings.push(format!(
                                "Step {}: Transform '{}' adds column '{}'",
                                i + 1,
                                transform_name,
                                col_name
                            ));
                        }
                    }
                }
            }
        }

        // Warning for transforms without schema info
        if transform.input_schema.is_none() && transform.output_schema.is_none() {
            result.warnings.push(format!(
                "Step {}: Transform '{}' has no schema metadata - cannot validate",
                i + 1,
                transform_name
            ));
        }
    }

    result
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
        print_endpoint(&project, &endpoint, false)?;
        return Ok(());
    }

    // Check committed endpoints
    if let Some(commit) = project.latest_commit()? {
        if let Some(endpoint) = commit.endpoints.get(name) {
            print_endpoint(&project, endpoint, true)?;
            return Ok(());
        }
    }

    anyhow::bail!("Endpoint '{}' not found", name);
}

fn print_endpoint(project: &Project, endpoint: &Endpoint, committed: bool) -> Result<()> {
    let status = if committed { "committed" } else { "staged" };
    println!("Endpoint: {} ({})", endpoint.name, status);

    if let Some(desc) = &endpoint.description {
        println!("Description: {}", desc);
    }

    println!();
    println!("Pipeline:");
    for edge in &endpoint.edges {
        let source = match edge.source_type {
            SourceType::DataSource => {
                // Show schema info for data sources
                let data_sources = commit::collect_data_sources(project)?;
                if let Some(ds) = data_sources.get(&edge.source_ref) {
                    let path = project.root.join(&ds.path);
                    if let Ok(schema_info) = schema::extract_parquet_schema(&path) {
                        format!("{} (data, {} columns)", edge.source_ref, schema_info.fields.len())
                    } else {
                        format!("{} (data)", edge.source_ref)
                    }
                } else {
                    format!("{} (data)", edge.source_ref)
                }
            }
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

    Ok(())
}
