use anyhow::Result;
use ozzy_core::project::{Endpoint, SourceType};
use ozzy_core::{cache, commit, hash, platform, runtime, Project};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

pub async fn execute(
    endpoint_name: &str,
    output: Option<&str>,
    force: bool,
    cli_params: &[(String, String)],
) -> Result<()> {
    let project = Project::find_current()?;

    // Find the endpoint
    let endpoint = find_endpoint(&project, endpoint_name)?;
    let data_sources = commit::collect_data_sources(&project)?;
    let transforms = commit::collect_transforms(&project)?;

    // Build params from CLI
    let params_override: serde_json::Value = if cli_params.is_empty() {
        serde_json::json!({})
    } else {
        let mut map = serde_json::Map::new();
        for (key, value) in cli_params {
            // Try to parse as JSON value, fall back to string
            let json_value = serde_json::from_str(value)
                .unwrap_or_else(|_| serde_json::Value::String(value.clone()));
            map.insert(key.clone(), json_value);
        }
        serde_json::Value::Object(map)
    };

    // Get platform fingerprint
    let platform = platform::PlatformFingerprint::detect();
    println!("Platform: {}", platform.short_string());
    println!();

    // Open the cache
    let local_cache = cache::LocalCache::open()?;

    // Build execution plan (topological order)
    let execution_order = build_execution_order(&endpoint);

    println!("Execution plan:");
    for node_name in &execution_order {
        let node = endpoint.nodes.iter().find(|n| n.node_name == *node_name).unwrap();
        println!("  {} (transform: {})", node_name, node.transform_name);
    }
    if !cli_params.is_empty() {
        println!();
        println!("Parameters:");
        for (key, value) in cli_params {
            println!("  {} = {}", key, value);
        }
    }
    println!();

    // Execute each node
    let mut node_outputs: HashMap<String, PathBuf> = HashMap::new();

    for node_name in &execution_order {
        let node = endpoint.nodes.iter().find(|n| n.node_name == *node_name).unwrap();
        let transform = transforms.get(&node.transform_name)
            .ok_or_else(|| anyhow::anyhow!("Transform '{}' not found", node.transform_name))?;

        // Find ALL input edges for this node (multi-input support)
        let input_edges: Vec<_> = endpoint.edges.iter()
            .filter(|e| e.target_node == *node_name)
            .collect();

        if input_edges.is_empty() {
            anyhow::bail!("No input edges for node '{}'", node_name);
        }

        // Build input paths HashMap
        let mut input_paths: HashMap<String, PathBuf> = HashMap::new();
        for edge in &input_edges {
            let input_path = match edge.source_type {
                SourceType::DataSource => {
                    let ds = data_sources.get(&edge.source_ref)
                        .ok_or_else(|| anyhow::anyhow!("Data source '{}' not found", edge.source_ref))?;
                    project.root.join(&ds.path)
                }
                SourceType::Node => {
                    node_outputs.get(&edge.source_ref)
                        .ok_or_else(|| anyhow::anyhow!("Node output '{}' not found", edge.source_ref))?
                        .clone()
                }
                SourceType::External => {
                    anyhow::bail!("External dependencies not yet supported in local execution");
                }
            };
            input_paths.insert(edge.input_name.clone(), input_path);
        }

        // Compute materialized hash from all inputs
        let mut input_hashes: Vec<String> = Vec::new();
        for (input_name, input_path) in &input_paths {
            let h = hash::blake3_hash_file(input_path)?;
            input_hashes.push(format!("{}:{}", input_name, h));
        }
        input_hashes.sort(); // Ensure deterministic order
        let combined_input_hash = hash::blake3_hash(input_hashes.join(",").as_bytes());

        // Merge node params with CLI params (CLI takes precedence)
        let effective_params = if params_override.is_object() && !params_override.as_object().unwrap().is_empty() {
            let mut merged = node.params.clone();
            if let (Some(base), Some(overrides)) = (merged.as_object_mut(), params_override.as_object()) {
                for (k, v) in overrides {
                    base.insert(k.clone(), v.clone());
                }
            }
            merged
        } else {
            node.params.clone()
        };

        let params_hash = hash::blake3_hash(effective_params.to_string().as_bytes());
        let materialized_hash = hash::materialized_hash(
            &combined_input_hash,
            &transform.hash,
            &params_hash,
            &platform.hash(),
        );

        println!("Executing: {}", node_name);
        if input_paths.len() == 1 {
            println!("  Input hash: {}...", &combined_input_hash[..12]);
        } else {
            println!("  Inputs: {} (combined hash: {}...)", input_paths.len(), &combined_input_hash[..12]);
        }
        println!("  Materialized hash: {}...", &materialized_hash[..12]);

        // Check cache
        let output_path = if !force {
            if let Some(cached_path) = local_cache.get_path(&materialized_hash)? {
                if cached_path.exists() {
                    println!("  Cache: HIT");
                    cached_path
                } else {
                    execute_node_multi(&project, transform, &input_paths, &effective_params, &materialized_hash, &local_cache, &platform)?
                }
            } else {
                execute_node_multi(&project, transform, &input_paths, &effective_params, &materialized_hash, &local_cache, &platform)?
            }
        } else {
            println!("  Cache: SKIP (forced)");
            execute_node_multi(&project, transform, &input_paths, &effective_params, &materialized_hash, &local_cache, &platform)?
        };

        node_outputs.insert(node_name.clone(), output_path);
    }

    // Get final output
    let final_node = execution_order.last().unwrap();
    let final_output = node_outputs.get(final_node).unwrap();

    // Copy to output location if specified
    if let Some(output_path) = output {
        fs::copy(final_output, output_path)?;
        println!();
        println!("Output written to: {}", output_path);
    } else {
        println!();
        println!("Output cached at: {}", final_output.display());
    }

    Ok(())
}

fn find_endpoint(project: &Project, name: &str) -> Result<Endpoint> {
    // Check staged endpoints first
    let staged_path = project.ozzy_dir().join("staged_endpoints").join(format!("{}.json", name));
    if staged_path.exists() {
        let content = fs::read_to_string(&staged_path)?;
        let endpoint: Endpoint = serde_json::from_str(&content)?;
        return Ok(endpoint);
    }

    // Check committed endpoints
    if let Some(commit) = project.latest_commit()? {
        if let Some(endpoint) = commit.endpoints.get(name) {
            return Ok(endpoint.clone());
        }
    }

    anyhow::bail!("Endpoint '{}' not found", name);
}

fn build_execution_order(endpoint: &Endpoint) -> Vec<String> {
    // Simple topological sort for linear pipelines
    // For more complex DAGs, would need proper topo sort
    let mut order = Vec::new();

    for node in &endpoint.nodes {
        order.push(node.node_name.clone());
    }

    order
}

/// Execute a node with multi-input support.
fn execute_node_multi(
    project: &Project,
    transform: &ozzy_core::project::Transform,
    input_paths: &HashMap<String, PathBuf>,
    params: &serde_json::Value,
    materialized_hash: &str,
    cache: &cache::LocalCache,
    platform: &platform::PlatformFingerprint,
) -> Result<PathBuf> {
    println!("  Cache: MISS - executing transform");

    let transform_path = project.root.join(&transform.source_path);

    // Create temp output file
    let temp_dir = tempfile::tempdir()?;
    let temp_output = temp_dir.path().join("output.parquet");

    // Execute the transform with multi-input support
    runtime::execute_transform_multi(
        &transform_path,
        &transform.function_name,
        input_paths,
        &temp_output,
        params,
    )?;

    // Store in cache
    let cached_path = cache.put(
        materialized_hash,
        &platform.short_string(),
        &temp_output,
        None, // row count
    )?;

    println!("  Cached at: {}", cached_path.display());

    Ok(cached_path)
}
