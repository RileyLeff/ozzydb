//! Fetch command for downloading and executing remote endpoints.

use anyhow::{Context, Result};
use ozzy_core::cache::LocalCache;
use ozzy_core::project::{Endpoint, SourceType};
use ozzy_core::registry::{CredentialsFile, RegistryClient};
use ozzy_core::{canon, commit, hash, platform, runtime};
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

/// Load credentials from config.
fn load_credentials() -> Result<CredentialsFile> {
    let path = dirs::config_dir()
        .context("Could not determine config directory")?
        .join("ozzy")
        .join("credentials.json");

    if path.exists() {
        let content = std::fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&content)?)
    } else {
        Ok(CredentialsFile::default())
    }
}

/// Parse a remote endpoint reference.
/// Format: [registry://]owner/project/endpoint[@ref]
fn parse_endpoint_ref(
    endpoint_ref: &str,
) -> Result<(Option<String>, String, String, String, String)> {
    // Check for explicit registry prefix
    let (registry, rest) = if let Some(idx) = endpoint_ref.find("://") {
        let registry_end = endpoint_ref[idx + 3..].find('/').map(|i| i + idx + 3);
        if let Some(end) = registry_end {
            (
                Some(endpoint_ref[..end].to_string()),
                &endpoint_ref[end + 1..],
            )
        } else {
            (None, endpoint_ref)
        }
    } else {
        (None, endpoint_ref)
    };

    // Parse owner/project/endpoint[@ref]
    let parts: Vec<&str> = rest.split('/').collect();
    if parts.len() < 3 {
        anyhow::bail!(
            "Invalid endpoint reference. Expected format: owner/project/endpoint[@ref] or registry://host/owner/project/endpoint[@ref]"
        );
    }

    let owner = parts[0].to_string();
    let project = parts[1].to_string();

    // Parse endpoint[@ref]
    let endpoint_part = parts[2..].join("/");
    let (endpoint, ref_name) = if let Some(at_idx) = endpoint_part.find('@') {
        (
            endpoint_part[..at_idx].to_string(),
            endpoint_part[at_idx + 1..].to_string(),
        )
    } else {
        (endpoint_part, "main".to_string())
    };

    Ok((registry, owner, project, endpoint, ref_name))
}

/// Get the default registry URL.
fn default_registry() -> String {
    std::env::var("OZZY_REGISTRY").unwrap_or_else(|_| "https://registry.ozzydb.dev".to_string())
}

fn parse_param_value(value: &str) -> serde_json::Value {
    serde_json::from_str(value).unwrap_or_else(|_| serde_json::Value::String(value.to_string()))
}

fn build_param_overrides(
    cli_params: &[(String, String)],
) -> (
    serde_json::Map<String, serde_json::Value>,
    HashMap<String, serde_json::Map<String, serde_json::Value>>,
) {
    let mut global = serde_json::Map::new();
    let mut scoped: HashMap<String, serde_json::Map<String, serde_json::Value>> = HashMap::new();

    for (key, raw_value) in cli_params {
        let value = parse_param_value(raw_value);
        if let Some((scope, param_name)) = key.split_once('.') {
            scoped
                .entry(scope.to_string())
                .or_default()
                .insert(param_name.to_string(), value);
        } else {
            global.insert(key.clone(), value);
        }
    }

    (global, scoped)
}

#[derive(Default)]
struct NocacheCleanup {
    paths: HashSet<PathBuf>,
}

impl NocacheCleanup {
    fn track(&mut self, path: &Path) {
        if is_nocache_output(path) {
            self.paths.insert(path.to_path_buf());
        }
    }

    fn cleanup(&mut self) -> Result<()> {
        let paths: Vec<PathBuf> = self.paths.drain().collect();
        for path in paths {
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "Failed to remove temporary nocache file {}: {}",
                        path.display(),
                        e
                    ));
                }
            }
        }
        Ok(())
    }
}

impl Drop for NocacheCleanup {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

fn is_nocache_output(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.starts_with("nocache_") && name.ends_with(".parquet"))
        .unwrap_or(false)
}

fn sanitize_archive_relative_path(path: &Path) -> Result<PathBuf> {
    let mut sanitized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => sanitized.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                anyhow::bail!("Refusing unsafe archive path: {}", path.display());
            }
        }
    }

    if sanitized.as_os_str().is_empty() {
        anyhow::bail!("Refusing empty archive path");
    }

    Ok(sanitized)
}

fn checked_destination(base: &Path, canonical_base: &Path, rel: &Path) -> Result<PathBuf> {
    let dest = base.join(rel);

    if let Some(parent) = dest.parent() {
        // Walk up to the nearest existing ancestor and validate it's within the
        // base BEFORE creating any directories, to prevent symlink traversal.
        let mut ancestor = parent.to_path_buf();
        while !ancestor.exists() {
            ancestor = ancestor
                .parent()
                .ok_or_else(|| anyhow::anyhow!("Invalid extraction path: {}", rel.display()))?
                .to_path_buf();
        }
        let canonical_ancestor = ancestor.canonicalize()?;
        if !canonical_ancestor.starts_with(canonical_base) {
            anyhow::bail!(
                "Archive extraction escaped destination root: {}",
                rel.display()
            );
        }

        std::fs::create_dir_all(parent)?;
        let canonical_parent = parent.canonicalize()?;
        if !canonical_parent.starts_with(canonical_base) {
            anyhow::bail!(
                "Archive extraction escaped destination root: {}",
                rel.display()
            );
        }
    }

    if dest.exists() {
        let canonical_dest = dest.canonicalize()?;
        if !canonical_dest.starts_with(canonical_base) {
            anyhow::bail!(
                "Archive extraction escaped destination root: {}",
                rel.display()
            );
        }
    }

    Ok(dest)
}

/// Set up a temp directory as a minimal ozzy project for execution.
fn setup_temp_project(temp_path: &std::path::Path, project_name: &str, owner: &str) -> Result<()> {
    let ozzy_dir = temp_path.join(".ozzy");
    std::fs::create_dir_all(ozzy_dir.join("commits"))?;
    std::fs::create_dir_all(ozzy_dir.join("refs").join("heads"))?;
    std::fs::create_dir_all(ozzy_dir.join("refs").join("tags"))?;
    std::fs::create_dir_all(ozzy_dir.join("objects").join("data"))?;
    std::fs::create_dir_all(ozzy_dir.join("objects").join("transforms"))?;
    std::fs::create_dir_all(temp_path.join("data"))?;
    std::fs::create_dir_all(temp_path.join("transforms"))?;

    let safe_name = project_name
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t");
    let safe_owner = owner
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t");
    let config = format!(
        "[project]\nname = \"{}\"\nowner = \"{}\"\n",
        safe_name, safe_owner
    );
    std::fs::write(temp_path.join("ozzy.toml"), config)?;

    Ok(())
}

/// Build execution order using Kahn's topological sort.
fn build_execution_order(endpoint: &Endpoint) -> anyhow::Result<Vec<String>> {
    use std::collections::{HashSet, VecDeque};

    let node_names: HashSet<String> = endpoint.nodes.iter().map(|n| n.node_name.clone()).collect();
    let mut dependencies: HashMap<String, HashSet<String>> = HashMap::new();
    for node in &endpoint.nodes {
        dependencies.insert(node.node_name.clone(), HashSet::new());
    }
    for edge in &endpoint.edges {
        if edge.source_type == SourceType::Node && node_names.contains(&edge.source_ref) {
            if let Some(deps) = dependencies.get_mut(&edge.target_node) {
                deps.insert(edge.source_ref.clone());
            }
        }
    }

    let mut in_degree: HashMap<String, usize> = HashMap::new();
    for (node, deps) in &dependencies {
        in_degree.insert(node.clone(), deps.len());
    }

    let mut queue: VecDeque<String> = VecDeque::new();
    for (node, &degree) in &in_degree {
        if degree == 0 {
            queue.push_back(node.clone());
        }
    }

    let mut order = Vec::new();
    while let Some(node) = queue.pop_front() {
        order.push(node.clone());
        for (other_node, deps) in &dependencies {
            if deps.contains(&node) {
                if let Some(degree) = in_degree.get_mut(other_node) {
                    *degree -= 1;
                    if *degree == 0 {
                        queue.push_back(other_node.clone());
                    }
                }
            }
        }
    }

    if order.len() != endpoint.nodes.len() {
        anyhow::bail!("Cycle detected in pipeline DAG. Cannot determine execution order.");
    }

    Ok(order)
}

/// Fetch and execute a remote endpoint.
pub async fn run(
    endpoint_ref: &str,
    output: Option<&str>,
    params: &[(String, String)],
    registry_override: Option<&str>,
) -> Result<()> {
    // Parse the endpoint reference
    let (registry_opt, owner, project, endpoint, ref_name) = parse_endpoint_ref(endpoint_ref)?;
    let registry = registry_opt
        .or_else(|| registry_override.map(|s| s.to_string()))
        .unwrap_or_else(default_registry);

    println!(
        "Fetching {}/{}/{}@{} from {}...",
        owner, project, endpoint, ref_name, registry
    );

    // Get credentials (optional for public projects)
    let creds = load_credentials().ok();
    let token = creds
        .as_ref()
        .and_then(|c| c.get(&registry))
        .map(|c| c.access_token.as_str());

    let client = if let Some(t) = token {
        RegistryClient::with_token(&registry, t)
    } else {
        RegistryClient::new(&registry)
    };

    // Get endpoint manifest to show what will be downloaded
    let manifest = client
        .fetch_manifest(&owner, &project, &endpoint, &ref_name)
        .await?;

    println!(
        "  Commit: {}",
        &manifest.commit_hash[..8.min(manifest.commit_hash.len())]
    );
    println!("  Data sources: {}", manifest.data_hashes.len());
    println!("  Transforms: {}", manifest.transform_hashes.len());

    // Download the endpoint content
    let tar_data = client.fetch(&owner, &project, &endpoint, &ref_name).await?;

    // Create a temporary directory and set up as minimal ozzy project
    let temp_dir = tempfile::tempdir()?;
    let temp_path = temp_dir.path();
    setup_temp_project(temp_path, &project, &owner)?;

    // Extract tar to temp directory
    let cursor = std::io::Cursor::new(tar_data);
    let mut archive = tar::Archive::new(cursor);

    let canonical_temp_path = temp_path.canonicalize()?;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let raw_path = entry.path()?.to_path_buf();
        let path = sanitize_archive_relative_path(&raw_path)?;
        let dest_path = checked_destination(temp_path, &canonical_temp_path, &path)?;

        let mut content = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut content)?;

        let mut file = std::fs::File::create(&dest_path)?;
        file.write_all(&content)?;
    }

    // Read and deserialize endpoint definition
    let endpoint_content = std::fs::read_to_string(temp_path.join("endpoint.json"))
        .context("Endpoint definition not found in archive")?;
    let endpoint_def: Endpoint =
        serde_json::from_str(&endpoint_content).context("Failed to parse endpoint definition")?;

    // Open the temp project and discover data sources / transforms
    let temp_project = ozzy_core::Project::open(temp_path)?;
    let data_sources = commit::collect_data_sources(&temp_project)?;
    let transforms = commit::collect_transforms(&temp_project)?;

    println!();
    println!("Executing endpoint locally...");

    // Build params from CLI
    let (global_param_overrides, scoped_param_overrides) = build_param_overrides(params);

    // Platform fingerprint and cache
    let plat = platform::PlatformFingerprint::detect();
    println!("Platform: {}", plat.short_string());

    let local_cache = LocalCache::open()?;

    // Build and display execution plan
    let execution_order = build_execution_order(&endpoint_def)?;
    println!("Execution plan:");
    for node_name in &execution_order {
        let node = endpoint_def
            .nodes
            .iter()
            .find(|n| n.node_name == *node_name)
            .ok_or_else(|| anyhow::anyhow!("Node '{}' not found in endpoint", node_name))?;
        println!("  {} (transform: {})", node_name, node.transform_name);
    }
    println!();

    // Execute each node
    let mut node_outputs: HashMap<String, PathBuf> = HashMap::new();
    let mut non_reproducible_nodes: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    let mut nocache_cleanup = NocacheCleanup::default();

    for node_name in &execution_order {
        let node = endpoint_def
            .nodes
            .iter()
            .find(|n| n.node_name == *node_name)
            .ok_or_else(|| anyhow::anyhow!("Node '{}' not found in endpoint", node_name))?;
        let transform = transforms.get(&node.transform_name).ok_or_else(|| {
            anyhow::anyhow!(
                "Transform '{}' not found in downloaded archive",
                node.transform_name
            )
        })?;

        // Find input edges
        let input_edges: Vec<_> = endpoint_def
            .edges
            .iter()
            .filter(|e| e.target_node == *node_name)
            .collect();

        if input_edges.is_empty() {
            anyhow::bail!("No input edges for node '{}'", node_name);
        }

        // Build input paths
        let mut input_paths: HashMap<String, PathBuf> = HashMap::new();
        for edge in &input_edges {
            let input_path = match edge.source_type {
                SourceType::DataSource => {
                    let ds = data_sources.get(&edge.source_ref).ok_or_else(|| {
                        anyhow::anyhow!(
                            "Data source '{}' not found in downloaded archive",
                            edge.source_ref
                        )
                    })?;
                    temp_project.root.join(&ds.path)
                }
                SourceType::Node => node_outputs
                    .get(&edge.source_ref)
                    .ok_or_else(|| {
                        anyhow::anyhow!("Node output '{}' not available yet", edge.source_ref)
                    })?
                    .clone(),
                SourceType::External => {
                    anyhow::bail!("External dependencies not yet supported in remote fetch");
                }
            };
            input_paths.insert(edge.input_name.clone(), input_path);
        }

        // Compute input hashes for all inputs
        let mut input_hash_pairs: Vec<(String, String)> = Vec::new();
        for (input_name, input_path) in &input_paths {
            let h = hash::blake3_hash_file(input_path)?;
            input_hash_pairs.push((input_name.clone(), h));
        }

        let has_global = !global_param_overrides.is_empty();
        let has_scoped = scoped_param_overrides.contains_key(&node.transform_name)
            || scoped_param_overrides.contains_key(&node.node_name);
        let effective_params = if has_global || has_scoped {
            let mut merged = node.params.clone();
            if let Some(base) = merged.as_object_mut() {
                for (k, v) in &global_param_overrides {
                    base.insert(k.clone(), v.clone());
                }
                if let Some(overrides) = scoped_param_overrides.get(&node.transform_name) {
                    for (k, v) in overrides {
                        base.insert(k.clone(), v.clone());
                    }
                }
                if let Some(overrides) = scoped_param_overrides.get(&node.node_name) {
                    for (k, v) in overrides {
                        base.insert(k.clone(), v.clone());
                    }
                }
            }
            merged
        } else {
            node.params.clone()
        };

        let params_hash = canon::hash_json(&effective_params);
        let params_schema_hash = canon::hash_json(&transform.params_schema);
        let full_transform_hash = hash::transform_hash(
            &transform.hash,
            &transform.function_name,
            &transform.lockfile_hash,
            &transform.runtime,
            &params_schema_hash,
        );

        // Use the proper multi-input hash function with \0-separated format
        let input_refs: Vec<(&str, &str)> = input_hash_pairs
            .iter()
            .map(|(n, h)| (n.as_str(), h.as_str()))
            .collect();
        let materialized_hash = hash::materialized_hash_multi_input(
            &input_refs,
            &full_transform_hash,
            &params_hash,
            &plat.hash(),
        );

        println!("Executing: {}", node_name);
        println!("  Materialized hash: {}...", &materialized_hash[..12]);

        // Check if this node inherits non-reproducibility from an upstream node
        let has_non_reproducible_upstream = endpoint_def
            .edges
            .iter()
            .filter(|e| e.target_node == *node_name && e.source_type == SourceType::Node)
            .any(|e| non_reproducible_nodes.contains(&e.source_ref));

        // Non-reproducible transforms (direct or inherited) always re-execute and don't cache
        let effectively_non_reproducible = !transform.reproducible || has_non_reproducible_upstream;
        let output_path = if effectively_non_reproducible {
            if !transform.reproducible {
                println!("  Cache: SKIP (non-reproducible transform)");
            } else {
                println!("  Cache: SKIP (non-reproducible upstream)");
            }
            execute_node_no_cache(&temp_project, transform, &input_paths, &effective_params).await?
        } else if let Some(cached_path) = local_cache.get_path(&materialized_hash)? {
            if cached_path.exists() {
                println!("  Cache: HIT");
                cached_path
            } else {
                execute_node(
                    &temp_project,
                    transform,
                    &input_paths,
                    &effective_params,
                    &materialized_hash,
                    &local_cache,
                    &plat,
                )
                .await?
            }
        } else {
            execute_node(
                &temp_project,
                transform,
                &input_paths,
                &effective_params,
                &materialized_hash,
                &local_cache,
                &plat,
            )
            .await?
        };

        if effectively_non_reproducible {
            non_reproducible_nodes.insert(node_name.clone());
        }
        nocache_cleanup.track(&output_path);

        node_outputs.insert(node_name.clone(), output_path);
    }

    // Get final output
    let final_node = execution_order.last().context("Empty execution plan")?;
    let final_output = node_outputs
        .get(final_node)
        .context("Final node output not found")?;

    // Copy to output location
    let output_path = if let Some(out) = output {
        PathBuf::from(out)
    } else {
        PathBuf::from(format!("{}-{}-{}.parquet", project, endpoint, ref_name))
    };

    std::fs::copy(final_output, &output_path)?;
    println!();
    println!("Output written to: {}", output_path.display());

    nocache_cleanup.cleanup()?;

    Ok(())
}

/// Validate output schema against transform's declared output_schema.
fn validate_output_schema(
    output_path: &std::path::Path,
    transform: &ozzy_core::project::Transform,
) -> Result<()> {
    let output_schema = match &transform.output_schema {
        Some(schema) => schema,
        None => return Ok(()),
    };

    let actual_schema = ozzy_core::schema::extract_parquet_schema(output_path)?;
    let actual_columns: std::collections::HashSet<String> = actual_schema
        .fields
        .iter()
        .map(|f| f.name.clone())
        .collect();

    if let Some(adds) = output_schema.get("adds").and_then(|v| v.as_array()) {
        for col in adds {
            if let Some(col_name) = col.as_str() {
                if !actual_columns.contains(col_name) {
                    anyhow::bail!(
                        "Output schema violation: transform '{}' declares output column '{}' but it was not found in output",
                        transform.name,
                        col_name
                    );
                }
            }
        }
    }

    if let Some(fields) = output_schema.get("fields").and_then(|v| v.as_array()) {
        for field in fields {
            if let Some(name) = field.get("name").and_then(|v| v.as_str()) {
                if !actual_columns.contains(name) {
                    anyhow::bail!(
                        "Output schema violation: transform '{}' declares output field '{}' but it was not found in output",
                        transform.name,
                        name
                    );
                }
            }
        }
    }

    Ok(())
}

/// Execute a single transform node.
async fn execute_node(
    project: &ozzy_core::Project,
    transform: &ozzy_core::project::Transform,
    input_paths: &HashMap<String, PathBuf>,
    params: &serde_json::Value,
    materialized_hash: &str,
    cache: &LocalCache,
    platform: &platform::PlatformFingerprint,
) -> Result<PathBuf> {
    println!("  Cache: MISS - executing transform");

    let transform_path = project.root.join(&transform.source_path);
    let temp_dir = tempfile::tempdir()?;
    let temp_output = temp_dir.path().join("output.parquet");

    runtime::execute_transform_multi(
        &transform_path,
        &transform.function_name,
        input_paths,
        &temp_output,
        params,
    )?;

    validate_output_schema(&temp_output, transform)?;

    let cached_path = cache.put(
        materialized_hash,
        &platform.short_string(),
        &temp_output,
        None,
    )?;

    println!("  Cached at: {}", cached_path.display());
    Ok(cached_path)
}

/// Execute a single transform node without caching (for non-reproducible transforms).
async fn execute_node_no_cache(
    project: &ozzy_core::Project,
    transform: &ozzy_core::project::Transform,
    input_paths: &HashMap<String, PathBuf>,
    params: &serde_json::Value,
) -> Result<PathBuf> {
    println!("  Executing transform (no cache)");

    let transform_path = project.root.join(&transform.source_path);
    let cache_dir = ozzy_core::cache::default_cache_dir();
    std::fs::create_dir_all(&cache_dir)?;
    let temp_output = cache_dir.join(format!(
        "nocache_{}.parquet",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));

    runtime::execute_transform_multi(
        &transform_path,
        &transform.function_name,
        input_paths,
        &temp_output,
        params,
    )?;

    validate_output_schema(&temp_output, transform)?;

    println!("  Output: {}", temp_output.display());
    Ok(temp_output)
}

#[cfg(test)]
mod tests {
    use super::sanitize_archive_relative_path;
    use std::path::Path;

    #[test]
    fn sanitize_archive_path_rejects_traversal() {
        assert!(sanitize_archive_relative_path(Path::new("../escape")).is_err());
        assert!(sanitize_archive_relative_path(Path::new("/absolute/path")).is_err());
    }

    #[test]
    fn sanitize_archive_path_allows_relative_paths() {
        let path = sanitize_archive_relative_path(Path::new("transforms/qc.py")).unwrap();
        assert_eq!(path.to_string_lossy(), "transforms/qc.py");
    }
}
