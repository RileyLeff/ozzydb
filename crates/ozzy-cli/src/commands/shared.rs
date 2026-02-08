//! Shared utilities used by multiple CLI commands.

use anyhow::{Context, Result};
use ozzy_core::cache::LocalCache;
use ozzy_core::project::{Endpoint, SourceType};
use ozzy_core::registry::CredentialsFile;
use ozzy_core::{canon, hash, platform, runtime, Project};
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

// ---------------------------------------------------------------------------
// Credentials
// ---------------------------------------------------------------------------

/// Load credentials from the user's config directory.
pub fn load_credentials() -> Result<CredentialsFile> {
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

/// Resolve an access token for a registry URL.
///
/// Priority: `OZZY_TOKEN` env var > credentials file.
/// Returns `None` if no token is available.
pub fn resolve_token(registry_url: &str) -> Option<String> {
    if let Ok(token) = std::env::var("OZZY_TOKEN") {
        if !token.is_empty() {
            return Some(token);
        }
    }
    load_credentials()
        .ok()
        .and_then(|c| c.get(registry_url).map(|r| r.access_token.clone()))
}

// ---------------------------------------------------------------------------
// Staged endpoint deletions
// ---------------------------------------------------------------------------

/// Load the set of endpoint names that have been staged for deletion.
pub fn load_staged_endpoint_deletions(project: &Project) -> Result<HashSet<String>> {
    let staged_dir = project.ozzy_dir().join("staged_endpoints");
    let mut deleted = HashSet::new();
    if !staged_dir.exists() {
        return Ok(deleted);
    }

    for entry in std::fs::read_dir(&staged_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().map(|e| e == "deleted").unwrap_or(false) {
            if let Some(stem) = path.file_stem() {
                deleted.insert(stem.to_string_lossy().to_string());
            }
        }
    }

    Ok(deleted)
}

// ---------------------------------------------------------------------------
// Archive extraction safety
// ---------------------------------------------------------------------------

/// Sanitize an archive-relative path, rejecting traversal attacks.
pub fn sanitize_archive_relative_path(path: &Path) -> Result<PathBuf> {
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

/// Validate and create a destination path for archive extraction.
pub fn checked_destination(base: &Path, canonical_base: &Path, rel: &Path) -> Result<PathBuf> {
    let dest = base.join(rel);

    if let Some(parent) = dest.parent() {
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
        // Reject symlinks to prevent following them to write outside the root.
        // Use symlink_metadata (does not follow symlinks) to detect this.
        let meta = dest.symlink_metadata()?;
        if meta.file_type().is_symlink() {
            anyhow::bail!(
                "Refusing to overwrite symlink during archive extraction: {}",
                rel.display()
            );
        }
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

// ---------------------------------------------------------------------------
// CLI parameter parsing
// ---------------------------------------------------------------------------

/// Parse a CLI parameter value as JSON, falling back to a string.
pub fn parse_param_value(value: &str) -> serde_json::Value {
    serde_json::from_str(value).unwrap_or_else(|_| serde_json::Value::String(value.to_string()))
}

/// Split CLI `--param` values into global and scoped (dot-separated) overrides.
pub fn build_param_overrides(
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

// ---------------------------------------------------------------------------
// Non-cached output management
// ---------------------------------------------------------------------------

/// Check whether a path is a temporary nocache output file.
pub fn is_nocache_output(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.starts_with("nocache_") && name.ends_with(".parquet"))
        .unwrap_or(false)
}

/// Tracks nocache output files for cleanup on success or drop.
#[derive(Default)]
pub struct NocacheCleanup {
    paths: HashSet<PathBuf>,
}

impl NocacheCleanup {
    pub fn track(&mut self, path: &Path) {
        if is_nocache_output(path) {
            self.paths.insert(path.to_path_buf());
        }
    }

    pub fn cleanup(&mut self) -> Result<()> {
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

// ---------------------------------------------------------------------------
// DAG execution
// ---------------------------------------------------------------------------

/// Build topological execution order using Kahn's algorithm.
pub fn build_execution_order(endpoint: &Endpoint) -> Result<Vec<String>> {
    use std::collections::VecDeque;

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

/// Validate output schema against transform's declared output_schema.
pub fn validate_output_schema(
    output_path: &Path,
    transform: &ozzy_core::project::Transform,
) -> Result<()> {
    let output_schema = match &transform.output_schema {
        Some(schema) => schema,
        None => return Ok(()),
    };

    let actual_schema = ozzy_core::schema::extract_parquet_schema(output_path)?;
    let actual_columns: HashSet<String> = actual_schema
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

/// Execute a transform node and cache the result.
pub async fn execute_node_cached(
    project: &Project,
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

/// Execute a transform node without caching (for non-reproducible transforms).
pub async fn execute_node_no_cache(
    project: &Project,
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

/// Execute nodes in a pipeline (the core execution loop shared by run and fetch).
pub async fn execute_pipeline(
    project: &Project,
    endpoint: &Endpoint,
    data_sources: &std::collections::BTreeMap<String, ozzy_core::project::DataSource>,
    transforms: &std::collections::BTreeMap<String, ozzy_core::project::Transform>,
    global_param_overrides: &serde_json::Map<String, serde_json::Value>,
    scoped_param_overrides: &HashMap<String, serde_json::Map<String, serde_json::Value>>,
    platform: &platform::PlatformFingerprint,
    local_cache: &LocalCache,
    force: bool,
) -> Result<(Vec<String>, HashMap<String, PathBuf>, NocacheCleanup)> {
    let execution_order = build_execution_order(endpoint)?;

    println!("Execution plan:");
    for node_name in &execution_order {
        let node = endpoint
            .nodes
            .iter()
            .find(|n| n.node_name == *node_name)
            .ok_or_else(|| anyhow::anyhow!("Node '{}' not found in endpoint", node_name))?;
        println!("  {} (transform: {})", node_name, node.transform_name);
    }
    println!();

    let mut node_outputs: HashMap<String, PathBuf> = HashMap::new();
    let mut non_reproducible_nodes: HashSet<String> = HashSet::new();
    let mut nocache_cleanup = NocacheCleanup::default();

    for node_name in &execution_order {
        let node = endpoint
            .nodes
            .iter()
            .find(|n| n.node_name == *node_name)
            .ok_or_else(|| anyhow::anyhow!("Node '{}' not found in endpoint", node_name))?;
        let transform = transforms
            .get(&node.transform_name)
            .ok_or_else(|| anyhow::anyhow!("Transform '{}' not found", node.transform_name))?;

        // Find input edges
        let input_edges: Vec<_> = endpoint
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
                        anyhow::anyhow!("Data source '{}' not found", edge.source_ref)
                    })?;
                    project.root.join(&ds.path)
                }
                SourceType::Node => node_outputs
                    .get(&edge.source_ref)
                    .ok_or_else(|| {
                        anyhow::anyhow!("Node output '{}' not available yet", edge.source_ref)
                    })?
                    .clone(),
                SourceType::External => {
                    anyhow::bail!("External dependencies not yet supported");
                }
            };
            input_paths.insert(edge.input_name.clone(), input_path);
        }

        // Compute input hashes
        let mut input_hash_pairs: Vec<(String, String)> = Vec::new();
        for (input_name, input_path) in &input_paths {
            let h = hash::blake3_hash_file(input_path)?;
            input_hash_pairs.push((input_name.clone(), h));
        }

        // Merge node params with CLI overrides
        let has_global = !global_param_overrides.is_empty();
        let has_scoped = scoped_param_overrides.contains_key(&node.transform_name)
            || scoped_param_overrides.contains_key(&node.node_name);
        let effective_params = if has_global || has_scoped {
            let mut merged = node.params.clone();
            if let Some(base) = merged.as_object_mut() {
                for (k, v) in global_param_overrides {
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

        // transform.hash already contains the full composite hash
        let input_refs: Vec<(&str, &str)> = input_hash_pairs
            .iter()
            .map(|(n, h)| (n.as_str(), h.as_str()))
            .collect();
        let materialized_hash = hash::materialized_hash_multi_input(
            &input_refs,
            &transform.hash,
            &params_hash,
            &platform.hash(),
        );

        println!("Executing: {}", node_name);
        if input_hash_pairs.len() == 1 {
            println!("  Input hash: {}...", input_hash_pairs[0].1.get(..12).unwrap_or(&input_hash_pairs[0].1));
        } else {
            println!("  Inputs: {}", input_hash_pairs.len());
        }
        println!("  Materialized hash: {}...", materialized_hash.get(..12).unwrap_or(&materialized_hash));

        // Check non-reproducibility
        let has_non_reproducible_upstream = endpoint
            .edges
            .iter()
            .filter(|e| e.target_node == *node_name && e.source_type == SourceType::Node)
            .any(|e| non_reproducible_nodes.contains(&e.source_ref));

        let effectively_non_reproducible = !transform.reproducible || has_non_reproducible_upstream;
        let output_path = if effectively_non_reproducible {
            if !transform.reproducible {
                println!("  Cache: SKIP (non-reproducible transform)");
            } else {
                println!("  Cache: SKIP (non-reproducible upstream)");
            }
            execute_node_no_cache(project, transform, &input_paths, &effective_params).await?
        } else if force {
            println!("  Cache: SKIP (forced)");
            execute_node_cached(
                project,
                transform,
                &input_paths,
                &effective_params,
                &materialized_hash,
                local_cache,
                platform,
            )
            .await?
        } else if let Some(cached_path) = local_cache.get_path(&materialized_hash)? {
            if cached_path.exists() {
                println!("  Cache: HIT");
                cached_path
            } else {
                execute_node_cached(
                    project,
                    transform,
                    &input_paths,
                    &effective_params,
                    &materialized_hash,
                    local_cache,
                    platform,
                )
                .await?
            }
        } else {
            execute_node_cached(
                project,
                transform,
                &input_paths,
                &effective_params,
                &materialized_hash,
                local_cache,
                platform,
            )
            .await?
        };

        if effectively_non_reproducible {
            non_reproducible_nodes.insert(node_name.clone());
        }
        nocache_cleanup.track(&output_path);
        node_outputs.insert(node_name.clone(), output_path);
    }

    Ok((execution_order, node_outputs, nocache_cleanup))
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
