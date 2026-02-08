//! DAG pipeline orchestration for server-side execution.
//!
//! This is the server-side equivalent of the CLI's `execute_pipeline()` in shared.rs.
//! Walks the endpoint DAG in topological order, checking the materialized cache at
//! each node, executing transforms in Docker on cache miss.

use std::collections::{BTreeMap, HashMap, VecDeque};

use anyhow::{Context, Result};
use uuid::Uuid;

use ozzy_core::canon;
use ozzy_core::hash;
use ozzy_core::platform::PlatformFingerprint;
use ozzy_core::project::{Endpoint, SourceType};

use crate::AppState;
use crate::db::models::{DbDataSource, DbTransform};

use super::{executor, image};

/// The platform fingerprint for server-side Docker execution.
///
/// Fixed and deterministic — does not detect the host. All server-side executions
/// on the same CPU architecture produce the same platform hash.
pub fn server_platform() -> PlatformFingerprint {
    PlatformFingerprint {
        os: "linux".to_string(),
        arch: std::env::consts::ARCH.to_string(),
        libc: Some("glibc-2.36".to_string()), // python:3.12-slim (Debian bookworm)
        cpu_features: vec![],                  // don't vary by CPU features in container
        blas: None,
        python_version: Some("3.12".to_string()),
    }
}

/// Build a topological execution order for the endpoint's nodes.
///
/// Returns node names in the order they should be executed (dependencies first).
pub fn build_execution_order(endpoint: &Endpoint) -> Result<Vec<String>> {
    let node_names: Vec<&str> = endpoint.nodes.iter().map(|n| n.node_name.as_str()).collect();

    // Build adjacency: for each node, which nodes depend on it?
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut in_degree: HashMap<&str, usize> = HashMap::new();

    for name in &node_names {
        dependents.entry(name).or_default();
        in_degree.entry(name).or_insert(0);
    }

    for edge in &endpoint.edges {
        if edge.source_type == SourceType::Node {
            // source_ref -> target_node dependency
            if let Some(deps) = dependents.get_mut(edge.source_ref.as_str()) {
                deps.push(&edge.target_node);
            }
            *in_degree.entry(edge.target_node.as_str()).or_insert(0) += 1;
        }
    }

    // Kahn's algorithm
    let mut queue: VecDeque<&str> = VecDeque::new();
    for name in &node_names {
        if in_degree[name] == 0 {
            queue.push_back(name);
        }
    }

    let mut order = Vec::new();
    while let Some(node) = queue.pop_front() {
        order.push(node.to_string());
        for dep in &dependents[node] {
            let deg = in_degree.get_mut(dep).unwrap();
            *deg -= 1;
            if *deg == 0 {
                queue.push_back(dep);
            }
        }
    }

    if order.len() != node_names.len() {
        anyhow::bail!("Cycle detected in endpoint DAG");
    }

    Ok(order)
}

/// Tracks the state of a node after execution (or cache hit).
struct NodeOutput {
    /// The materialized hash for this node's output.
    materialized_hash: String,
    /// Hash of the output content itself (for use as input to downstream nodes).
    content_hash: String,
}

/// Execute an endpoint pipeline, returning the materialized hash of the final output.
///
/// For each node in topological order:
/// 1. Compute the materialized hash
/// 2. Check cache (DB + R2)
/// 3. On miss: download inputs, execute transform in Docker, store result
///
/// Returns the materialized hash of the terminal node.
pub async fn execute_endpoint(
    state: &AppState,
    project_id: Uuid,
    commit_hash: &str,
    endpoint: &Endpoint,
    data_sources: &[DbDataSource],
    transforms: &[DbTransform],
) -> Result<String> {
    let platform = server_platform();
    let platform_hash = platform.hash();

    // Index data sources and transforms by name
    let ds_map: BTreeMap<&str, &DbDataSource> =
        data_sources.iter().map(|ds| (ds.name.as_str(), ds)).collect();
    let tx_map: BTreeMap<&str, &DbTransform> =
        transforms.iter().map(|t| (t.name.as_str(), t)).collect();

    let order = build_execution_order(endpoint)?;

    // Map node_name -> NodeOutput for resolved outputs
    let mut node_outputs: HashMap<String, NodeOutput> = HashMap::new();

    // Find the terminal node (last in topo order)
    let terminal_node = order
        .last()
        .ok_or_else(|| anyhow::anyhow!("Endpoint has no nodes"))?
        .clone();

    for node_name in &order {
        let node = endpoint
            .nodes
            .iter()
            .find(|n| &n.node_name == node_name)
            .ok_or_else(|| anyhow::anyhow!("Node '{}' not found in endpoint", node_name))?;

        let transform = tx_map
            .get(node.transform_name.as_str())
            .ok_or_else(|| {
                anyhow::anyhow!("Transform '{}' not found for node '{}'", node.transform_name, node_name)
            })?;

        // Resolve input hashes for this node
        let mut input_refs: Vec<(&str, String)> = Vec::new();
        for edge in &endpoint.edges {
            if edge.target_node != *node_name {
                continue;
            }
            let input_hash = match edge.source_type {
                SourceType::DataSource => {
                    let ds = ds_map.get(edge.source_ref.as_str()).ok_or_else(|| {
                        anyhow::anyhow!("Data source '{}' not found", edge.source_ref)
                    })?;
                    ds.content_hash.clone()
                }
                SourceType::Node => {
                    let upstream = node_outputs.get(&edge.source_ref).ok_or_else(|| {
                        anyhow::anyhow!(
                            "Upstream node '{}' not yet executed (topological sort error?)",
                            edge.source_ref
                        )
                    })?;
                    upstream.content_hash.clone()
                }
                SourceType::External => {
                    anyhow::bail!("External dependencies not yet supported in server-side compute");
                }
            };
            input_refs.push((edge.input_name.as_str(), input_hash));
        }

        // Compute composite transform hash (same as ozzy-core commit.rs)
        let params_schema_hash = canon::hash_json(&transform.params_schema);
        let composite_hash = hash::transform_hash(
            &transform.content_hash,
            &transform.function_name,
            &transform.lockfile_hash,
            &transform.runtime,
            &params_schema_hash,
        );

        // Merge node params with transform defaults
        let effective_params = &node.params;
        let params_hash = canon::hash_json(effective_params);

        // Compute materialized hash
        let input_ref_strs: Vec<(&str, &str)> = input_refs
            .iter()
            .map(|(name, hash)| (*name, hash.as_str()))
            .collect();
        let materialized_hash = hash::materialized_hash_multi_input(
            &input_ref_strs,
            &composite_hash,
            &params_hash,
            &platform_hash,
        );

        // Check cache: DB lookup first
        let cache_entry = state
            .db
            .get_materialized_cache_entry(&materialized_hash)
            .await?;

        if let Some(entry) = cache_entry {
            // Cache hit — verify R2 has the content
            if state
                .materialized_storage
                .exists(&materialized_hash, "parquet")
                .await?
            {
                tracing::debug!(
                    "Cache hit for node '{}': {}",
                    node_name,
                    &materialized_hash[..12]
                );
                // Bump access count
                state
                    .db
                    .upsert_materialized_cache_entry(
                        &materialized_hash,
                        project_id,
                        &endpoint.name,
                        commit_hash,
                        &platform_hash,
                        entry.byte_size,
                        entry.row_count,
                    )
                    .await?;

                node_outputs.insert(
                    node_name.clone(),
                    NodeOutput {
                        materialized_hash: materialized_hash.clone(),
                        content_hash: materialized_hash.clone(),
                    },
                );
                continue;
            }
            // DB entry exists but R2 content is missing — re-execute
            tracing::warn!(
                "Stale cache entry for node '{}': DB hit but R2 miss",
                node_name
            );
        }

        // Cache miss — execute the transform
        tracing::info!(
            "Executing node '{}' (transform: {})",
            node_name,
            transform.name
        );

        // Create temp directories for inputs and output
        let input_dir =
            tempfile::tempdir().context("Failed to create input temp dir")?;
        let output_dir =
            tempfile::tempdir().context("Failed to create output temp dir")?;

        // Download inputs to input_dir
        for (input_name, input_hash) in &input_refs {
            let content = if let Some(upstream) = node_outputs.get(*input_name).or_else(|| {
                // input_name is the edge's input_name, but we need to check source_ref
                // for node outputs. Find the right source.
                None
            }) {
                // Upstream node output — stored in materialized storage
                state
                    .materialized_storage
                    .get(&upstream.materialized_hash, "parquet")
                    .await
                    .with_context(|| {
                        format!("Failed to fetch upstream output '{}'", input_name)
                    })?
            } else {
                // Check if this is a data source or upstream node output
                let is_upstream_node = endpoint.edges.iter().any(|e| {
                    e.target_node == *node_name
                        && e.input_name == *input_name
                        && e.source_type == SourceType::Node
                });
                if is_upstream_node {
                    // Find the upstream node by source_ref
                    let source_ref = endpoint
                        .edges
                        .iter()
                        .find(|e| {
                            e.target_node == *node_name
                                && e.input_name == *input_name
                                && e.source_type == SourceType::Node
                        })
                        .map(|e| &e.source_ref)
                        .unwrap();
                    let upstream = node_outputs.get(source_ref.as_str()).ok_or_else(|| {
                        anyhow::anyhow!("Upstream node '{}' output not found", source_ref)
                    })?;
                    state
                        .materialized_storage
                        .get(&upstream.materialized_hash, "parquet")
                        .await
                        .with_context(|| {
                            format!("Failed to fetch upstream output '{}'", source_ref)
                        })?
                } else {
                    // Data source — stored in content storage
                    state
                        .storage
                        .get(input_hash, "parquet")
                        .await
                        .with_context(|| {
                            format!("Failed to fetch data source '{}'", input_name)
                        })?
                }
            };

            let input_path = input_dir.path().join(format!("{}.parquet", input_name));
            tokio::fs::write(&input_path, &content)
                .await
                .with_context(|| format!("Failed to write input '{}'", input_name))?;
        }

        // Download transform source
        let transform_source = state
            .storage
            .get(&transform.content_hash, "py")
            .await
            .with_context(|| format!("Failed to fetch transform source '{}'", transform.name))?;

        // Ensure Docker image exists
        let empty_hash = ozzy_core::hash::blake3_hash(b"");
        let image_tag =
            if !transform.lockfile_hash.is_empty() && transform.lockfile_hash != empty_hash {
                // Has a lockfile — try to fetch it and build image
                match state.storage.get(&transform.lockfile_hash, "lock").await {
                    Ok(lockfile_bytes) => {
                        let lockfile_content = String::from_utf8_lossy(&lockfile_bytes);
                        image::ensure_lockfile_image(
                            &transform.lockfile_hash,
                            &lockfile_content,
                        )
                        .await?
                    }
                    Err(_) => {
                        tracing::warn!(
                            "Lockfile {} not found in storage, using base image",
                            &transform.lockfile_hash[..12]
                        );
                        image::ensure_base_image().await?
                    }
                }
            } else {
                image::ensure_base_image().await?
            };

        // Execute
        executor::execute_transform(
            &state.config.compute,
            &image_tag,
            &transform_source,
            &transform.function_name,
            input_dir.path(),
            output_dir.path(),
            effective_params,
        )
        .await
        .with_context(|| format!("Failed to execute node '{}'", node_name))?;

        // Read output and store in materialized storage
        let output_path = output_dir.path().join("output.parquet");
        let output_bytes = tokio::fs::read(&output_path)
            .await
            .context("Failed to read transform output")?;

        let byte_size = output_bytes.len() as i64;

        // Store in R2 (materialized storage uses content hash = materialized hash)
        // We use the materialized hash as the storage key since that's the cache key.
        state
            .materialized_storage
            .store_with_hash(&materialized_hash, &output_bytes, "parquet")
            .await
            .context("Failed to store materialized output")?;

        // Record in DB
        state
            .db
            .upsert_materialized_cache_entry(
                &materialized_hash,
                project_id,
                &endpoint.name,
                commit_hash,
                &platform_hash,
                byte_size,
                None, // TODO: extract row count from parquet metadata
            )
            .await?;

        tracing::info!(
            "Node '{}' complete: {} ({} bytes)",
            node_name,
            &materialized_hash[..12],
            byte_size,
        );

        node_outputs.insert(
            node_name.clone(),
            NodeOutput {
                materialized_hash: materialized_hash.clone(),
                content_hash: materialized_hash.clone(),
            },
        );
    }

    // Return the terminal node's materialized hash
    let terminal = node_outputs
        .get(&terminal_node)
        .ok_or_else(|| anyhow::anyhow!("Terminal node '{}' has no output", terminal_node))?;

    Ok(terminal.materialized_hash.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ozzy_core::project::{PipelineEdge, PipelineNode};

    fn simple_endpoint() -> Endpoint {
        Endpoint {
            name: "test".to_string(),
            description: None,
            nodes: vec![PipelineNode {
                node_name: "clean".to_string(),
                transform_name: "cleaner".to_string(),
                params: serde_json::json!({}),
            }],
            edges: vec![PipelineEdge {
                target_node: "clean".to_string(),
                input_name: "main".to_string(),
                source_type: SourceType::DataSource,
                source_ref: "raw_data".to_string(),
                external_owner: None,
                external_project: None,
                external_endpoint: None,
                external_commit_hash: None,
            }],
        }
    }

    fn multi_node_endpoint() -> Endpoint {
        Endpoint {
            name: "pipeline".to_string(),
            description: None,
            nodes: vec![
                PipelineNode {
                    node_name: "clean".to_string(),
                    transform_name: "cleaner".to_string(),
                    params: serde_json::json!({}),
                },
                PipelineNode {
                    node_name: "aggregate".to_string(),
                    transform_name: "aggregator".to_string(),
                    params: serde_json::json!({}),
                },
            ],
            edges: vec![
                PipelineEdge {
                    target_node: "clean".to_string(),
                    input_name: "main".to_string(),
                    source_type: SourceType::DataSource,
                    source_ref: "raw_data".to_string(),
                    external_owner: None,
                    external_project: None,
                    external_endpoint: None,
                    external_commit_hash: None,
                },
                PipelineEdge {
                    target_node: "aggregate".to_string(),
                    input_name: "main".to_string(),
                    source_type: SourceType::Node,
                    source_ref: "clean".to_string(),
                    external_owner: None,
                    external_project: None,
                    external_endpoint: None,
                    external_commit_hash: None,
                },
            ],
        }
    }

    #[test]
    fn test_build_execution_order_single_node() {
        let ep = simple_endpoint();
        let order = build_execution_order(&ep).unwrap();
        assert_eq!(order, vec!["clean"]);
    }

    #[test]
    fn test_build_execution_order_multi_node() {
        let ep = multi_node_endpoint();
        let order = build_execution_order(&ep).unwrap();
        assert_eq!(order, vec!["clean", "aggregate"]);
    }

    #[test]
    fn test_build_execution_order_detects_cycle() {
        let ep = Endpoint {
            name: "cycle".to_string(),
            description: None,
            nodes: vec![
                PipelineNode {
                    node_name: "a".to_string(),
                    transform_name: "t".to_string(),
                    params: serde_json::json!({}),
                },
                PipelineNode {
                    node_name: "b".to_string(),
                    transform_name: "t".to_string(),
                    params: serde_json::json!({}),
                },
            ],
            edges: vec![
                PipelineEdge {
                    target_node: "b".to_string(),
                    input_name: "main".to_string(),
                    source_type: SourceType::Node,
                    source_ref: "a".to_string(),
                    external_owner: None,
                    external_project: None,
                    external_endpoint: None,
                    external_commit_hash: None,
                },
                PipelineEdge {
                    target_node: "a".to_string(),
                    input_name: "main".to_string(),
                    source_type: SourceType::Node,
                    source_ref: "b".to_string(),
                    external_owner: None,
                    external_project: None,
                    external_endpoint: None,
                    external_commit_hash: None,
                },
            ],
        };
        assert!(build_execution_order(&ep).is_err());
    }

    #[test]
    fn test_server_platform_deterministic() {
        let p1 = server_platform();
        let p2 = server_platform();
        assert_eq!(p1.hash(), p2.hash());
    }

    #[test]
    fn test_server_platform_is_linux() {
        let p = server_platform();
        assert_eq!(p.os, "linux");
        assert_eq!(p.python_version, Some("3.12".to_string()));
    }
}
