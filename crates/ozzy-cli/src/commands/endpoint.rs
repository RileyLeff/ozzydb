use anyhow::Result;
use ozzy_core::project::{Endpoint, PipelineEdge, PipelineNode, SourceType};
use ozzy_core::{commit, schema, Project};
use std::collections::HashMap;
use std::fs;

/// Create an endpoint with multi-input support.
/// inputs is a list of (input_name, data_source_name) pairs.
pub async fn create(name: &str, inputs: &[(String, String)], transforms: &[String]) -> Result<()> {
    let mut project = Project::find_current()?;

    if inputs.is_empty() {
        anyhow::bail!("At least one input is required");
    }

    // Verify all input data sources exist
    let data_sources = commit::collect_data_sources(&project)?;
    let mut input_schemas: HashMap<String, schema::SchemaInfo> = HashMap::new();

    println!("Validating pipeline schema...");
    println!();

    for (input_name, source_name) in inputs {
        let data_source = data_sources.get(source_name).ok_or_else(|| {
            anyhow::anyhow!(
                "Data source '{}' not found. Available: {}",
                source_name,
                data_sources.keys().cloned().collect::<Vec<_>>().join(", ")
            )
        })?;

        let data_path = project.root.join(&data_source.path);
        let input_schema = schema::extract_parquet_schema(&data_path)?;

        println!("Input '{}' from '{}' schema:", input_name, source_name);
        for field in &input_schema.fields {
            let nullable = if field.nullable { "?" } else { "" };
            println!("  {}: {}{}", field.name, field.dtype, nullable);
        }
        println!();

        input_schemas.insert(input_name.clone(), input_schema);
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

    // Validate schema compatibility for all inputs
    // The first transform gets all input schemas; subsequent transforms get chained output
    // For the initial validation, use "main" or the first input for pipeline propagation
    let primary_schema = input_schemas.get("main").or_else(|| input_schemas.values().next());
    if let Some(schema) = primary_schema {
        let validation_result = validate_pipeline_schema(schema, transforms, &available_transforms);

        // Also validate that all named inputs match transform requirements
        if !transforms.is_empty() {
            if let Some(first_transform) = available_transforms.get(&transforms[0]) {
                if let Some(input_schema_value) = &first_transform.input_schema {
                    if let Some(required_inputs) = input_schema_value.get("inputs") {
                        if let Some(required_list) = required_inputs.as_array() {
                            for req in required_list {
                                if let Some(req_name) = req.as_str() {
                                    if !input_schemas.contains_key(req_name) {
                                        println!("  ✗ Transform '{}' expects input '{}' but it was not provided",
                                            transforms[0], req_name);
                                        anyhow::bail!("Pipeline validation failed. Fix schema issues before creating endpoint.");
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

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
    }

    println!("  ✓ Schema validation passed");
    println!();

    // Build the pipeline nodes and edges
    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    // Track previous outputs for chaining
    let mut prev_outputs: HashMap<String, (String, SourceType)> = HashMap::new();

    // Initially, all inputs come from data sources
    for (input_name, source_name) in inputs {
        prev_outputs.insert(input_name.clone(), (source_name.clone(), SourceType::DataSource));
    }

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

        // For the first node, create edges from all data sources
        // For subsequent nodes, chain from previous node's output
        if i == 0 {
            for (input_name, source_name) in inputs {
                edges.push(PipelineEdge {
                    target_node: node_name.clone(),
                    input_name: input_name.clone(),
                    source_type: SourceType::DataSource,
                    source_ref: source_name.clone(),
                    external_owner: None,
                    external_project: None,
                    external_endpoint: None,
                    external_commit_hash: None,
                });
            }
        } else {
            // Chain from previous node - pass output as "main" input
            let prev_node = &nodes[i - 1].node_name;
            edges.push(PipelineEdge {
                target_node: node_name.clone(),
                input_name: "main".to_string(),
                source_type: SourceType::Node,
                source_ref: prev_node.clone(),
                external_owner: None,
                external_project: None,
                external_endpoint: None,
                external_commit_hash: None,
            });
        }
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
    for (input_name, source_name) in inputs {
        println!("  {} -> [{}] (data source)", source_name, input_name);
    }
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
    available_transforms: &std::collections::BTreeMap<String, ozzy_core::project::Transform>,
) -> schema::ValidationResult {
    let mut result = schema::ValidationResult::ok();

    // Track current schema (starts with input data source schema)
    let mut current_columns: Vec<String> = input_schema.column_names().iter().map(|s| s.to_string()).collect();

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
                    // requires: ["col1", "col2"] - name-only check
                    for col in required_cols {
                        if let Some(col_name) = col.as_str() {
                            if !current_columns.iter().any(|c| c == col_name) {
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
                } else if let Some(required_map) = required.as_object() {
                    // requires: {"col1": "float64", "col2": "utf8"} - name + type check
                    for (col_name, expected_type) in required_map {
                        if !current_columns.iter().any(|c| c == col_name) {
                            result.valid = false;
                            result.errors.push(format!(
                                "Step {}: Transform '{}' requires column '{}' which is not available. Available: {:?}",
                                i + 1,
                                transform_name,
                                col_name,
                                current_columns
                            ));
                        } else if let Some(expected_type_str) = expected_type.as_str() {
                            // Check type compatibility against input schema fields
                            if let Some(field) = input_schema.fields.iter().find(|f| f.name == *col_name) {
                                if !types_compatible(&field.dtype, expected_type_str) {
                                    result.warnings.push(format!(
                                        "Step {}: Transform '{}' expects column '{}' to be '{}' but found '{}'",
                                        i + 1,
                                        transform_name,
                                        col_name,
                                        expected_type_str,
                                        field.dtype
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }

        // Track columns added by this transform's output schema
        if let Some(output_schema_value) = &transform.output_schema {
            if let Some(adds) = output_schema_value.get("adds") {
                if let Some(added_cols) = adds.as_array() {
                    for col in added_cols {
                        if let Some(col_name) = col.as_str() {
                            if !current_columns.iter().any(|c| c == col_name) {
                                current_columns.push(col_name.to_string());
                            }
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
            // Handle "drops" if present
            if let Some(drops) = output_schema_value.get("drops") {
                if let Some(dropped_cols) = drops.as_array() {
                    for col in dropped_cols {
                        if let Some(col_name) = col.as_str() {
                            current_columns.retain(|c| c != col_name);
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

/// Check if two type strings are compatible.
/// Uses flexible matching: "float64" matches "number", "int64" matches "integer", etc.
fn types_compatible(actual: &str, expected: &str) -> bool {
    if actual == expected {
        return true;
    }
    // Normalize Python/JSON-schema type names to Arrow type names
    fn normalize(t: &str) -> &str {
        match t {
            "number" | "float" => "float64",
            "integer" | "int" => "int64",
            "string" | "str" => "utf8",
            "boolean" | "bool" => "bool",
            other => other,
        }
    }
    let actual_norm = normalize(actual);
    let expected_norm = normalize(expected);
    if actual_norm == expected_norm {
        return true;
    }
    // Numeric compatibility: int types are compatible with float expectations
    fn is_numeric(t: &str) -> bool {
        matches!(t, "int8" | "int16" | "int32" | "int64" | "uint8" | "uint16" | "uint32" | "uint64"
            | "float16" | "float32" | "float64")
    }
    if expected_norm == "float64" && is_numeric(actual_norm) {
        return true;
    }
    false
}
