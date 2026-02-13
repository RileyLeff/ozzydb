//! `ozzy run <endpoint>` — local DAG execution using Docker.
//!
//! Reads `ozzy.toml` from the local working directory (not git), resolves data
//! references, builds a topological execution plan, and runs each transform in
//! a Docker container. Results are cached in `~/.ozzy/cache/materialized/`.
//!
//! This is the local development counterpart of the server's fetch endpoint.
//! The key difference: source files come from the local filesystem (fast iteration),
//! not from a committed git snapshot.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use tokio::process::Command;

use ozzy_core::hash;
use ozzy_core::platform::PlatformFingerprint;
use ozzy_core::toml_spec::{
    EdgeSource, EndpointDef, EnvironmentDef, EnvironmentTier, OzzyToml, TransformDef,
    parse_edge_source, parse_edge_target,
};

// ============================================================================
// Types
// ============================================================================

struct NodeOutput {
    #[allow(dead_code)]
    hash: String,
    /// blake3 of the primary output file content (used as input hash for downstream nodes)
    output_hash: String,
    output_dir: PathBuf,
}

struct InputFile {
    name: String,
    local_path: PathBuf,
    content_type: String,
    is_collection: bool,
}

// ============================================================================
// Entry point
// ============================================================================

/// Execute `ozzy run <endpoint>`.
pub async fn run(
    cwd: &Path,
    endpoint_name: &str,
    output: Option<&str>,
    force: bool,
    params: &[String],
    local_data: &[String],
) -> Result<()> {
    // 1. Parse ozzy.toml
    let toml_path = cwd.join("ozzy.toml");
    let toml_content = std::fs::read_to_string(&toml_path)
        .context("No ozzy.toml found. Run `ozzy init` first.")?;
    let spec = OzzyToml::parse(&toml_content).context("Failed to parse ozzy.toml")?;

    // 2. Validate
    let errors = spec.validate();
    if !errors.is_empty() {
        for e in &errors {
            eprintln!("  {}", e);
        }
        bail!("ozzy.toml has {} validation error(s)", errors.len());
    }

    // 3. Find endpoint
    let endpoint = spec
        .endpoints
        .get(endpoint_name)
        .ok_or_else(|| anyhow::anyhow!("Endpoint '{}' not found in ozzy.toml", endpoint_name))?;

    // 4. Parse user params and --local-data
    let user_params = parse_param_args(params)?;
    let local_data_map = parse_local_data_args(local_data)?;

    // 5. Resolve endpoint params (apply overrides + defaults)
    let resolved_params = resolve_endpoint_params(endpoint, &user_params)?;

    // 6. Build execution order (topological sort)
    let execution_order = build_execution_order(endpoint)?;
    if execution_order.is_empty() {
        bail!("Endpoint '{}' has no nodes", endpoint_name);
    }

    // 7. Build edge map: node → [(input_name, EdgeSource)]
    let edge_map = build_edge_map(endpoint);

    // 8. Detect platform
    let platform = PlatformFingerprint::detect();
    let platform_hash = platform.hash();

    // 9. Setup cache dir
    let cache_dir = dirs::home_dir()
        .context("Cannot determine home directory")?
        .join(".ozzy")
        .join("cache")
        .join("materialized");
    std::fs::create_dir_all(&cache_dir)?;

    eprintln!(
        "Running endpoint '{}' ({} node(s))",
        endpoint_name,
        execution_order.len()
    );

    // 10. Execute DAG
    let mut node_outputs: HashMap<String, NodeOutput> = HashMap::new();

    for node_name in &execution_order {
        let node = &endpoint.nodes[node_name];
        let transform = spec.transforms.get(&node.transform).ok_or_else(|| {
            anyhow::anyhow!(
                "Transform '{}' referenced by node '{}' not found",
                node.transform,
                node_name
            )
        })?;
        let env_def = spec
            .environments
            .get(&transform.environment)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Environment '{}' referenced by transform '{}' not found",
                    transform.environment,
                    node.transform
                )
            })?;

        // Resolve node params (static params + endpoint param binds)
        let node_params = resolve_node_params(node_name, &node.params, &resolved_params, endpoint);
        let params_json = serde_json::to_string(&node_params)?;
        let params_hash = hash::blake3_hash(params_json.as_bytes());

        // Compute source hash from local filesystem
        let source_hash = compute_source_hash(cwd, transform)?;

        // Compute environment hash
        let env_hash = compute_env_hash(cwd, env_def)?;

        // Compute lockfile hash
        let lockfile_hash = compute_lockfile_hash(cwd, env_def);

        // Compute params schema hash (deterministic hash of the param definitions)
        let params_schema_hash = compute_params_schema_hash(transform);

        // Function name
        let function_name = transform
            .source
            .as_deref()
            .and_then(|s| s.rsplit_once(':').map(|(_, f)| f))
            .unwrap_or("command");

        // Compute transform hash
        let t_hash = hash::transform_hash(
            &source_hash,
            function_name,
            &lockfile_hash,
            &env_hash,
            &params_schema_hash,
        );

        // Resolve input hashes
        let input_hashes =
            resolve_input_hashes(node_name, &edge_map, &local_data_map, &node_outputs)?;

        // Compute materialized hash (no secrets for local execution)
        let input_refs: Vec<(&str, &str)> = input_hashes
            .iter()
            .map(|(n, h)| (n.as_str(), h.as_str()))
            .collect();
        let mat_hash =
            hash::materialized_hash(&input_refs, &t_hash, &params_hash, &platform_hash, None);

        // Check cache
        let cache_path = cache_dir.join(&mat_hash);
        if cache_path.exists() && !force {
            let short = mat_hash.get(..12).unwrap_or(&mat_hash);
            eprintln!("  {} — cache hit ({})", node_name, short);
            let primary = find_primary_output(&cache_path)?;
            let output_hash = hash::blake3_hash_file(&primary)?;
            node_outputs.insert(
                node_name.clone(),
                NodeOutput {
                    hash: mat_hash,
                    output_hash,
                    output_dir: cache_path,
                },
            );
            continue;
        }

        // Need to execute
        let short = mat_hash.get(..12).unwrap_or(&mat_hash);
        eprintln!("  {} — executing ({})", node_name, short);
        let start = Instant::now();

        // Resolve environment image (build if needed)
        let image = resolve_local_environment(cwd, env_def, &env_hash).await?;

        // Build input files list
        let input_files = resolve_input_files(
            node_name,
            transform,
            &edge_map,
            &local_data_map,
            &node_outputs,
        )?;

        // Generate runner script
        let (runner_script, runner_ext) = generate_runner(transform)?;

        // Generate init script
        let init_script = generate_init_script(transform);

        // Build env vars
        let mut env_vars: HashMap<String, String> = HashMap::new();
        let manifest = build_input_manifest(&input_files);
        env_vars.insert("OZZY_INPUT_MANIFEST".to_string(), manifest.to_string());
        env_vars.insert("OZZY_PARAMS".to_string(), params_json.clone());
        if let Some(obj) = node_params.as_object() {
            for (key, value) in obj {
                let str_value = match value {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                env_vars.insert(format!("OZZY_PARAM_{}", key), str_value);
            }
        }

        // Source directory for Python/R transforms (bind mount the project root)
        let source_dir = if transform.source.is_some() {
            Some(cwd)
        } else {
            None
        };

        // Create temp workspace
        let workspace = tempfile::tempdir().context("Failed to create temp workspace")?;
        let workspace_path = workspace.path();

        // Write runner + init scripts
        let runner_path = workspace_path.join(format!("runner.{}", runner_ext));
        std::fs::write(&runner_path, &runner_script)?;
        let init_path = workspace_path.join("init.sh");
        std::fs::write(&init_path, &init_script)?;

        // Create workspace subdirs
        let ws_inputs = workspace_path.join("inputs");
        let ws_output = workspace_path.join("output");
        let ws_source = workspace_path.join("source");
        std::fs::create_dir_all(&ws_inputs)?;
        std::fs::create_dir_all(&ws_output)?;
        std::fs::create_dir_all(&ws_source)?;

        // Link/copy inputs to workspace
        for input in &input_files {
            let dest = ws_inputs.join(&input.name);
            if input.is_collection {
                copy_dir_sync(&input.local_path, &dest)?;
            } else {
                // Try hard link, fall back to copy
                if std::fs::hard_link(&input.local_path, &dest).is_err() {
                    std::fs::copy(&input.local_path, &dest)?;
                }
            }
        }

        // If source-based transform, copy source dir (or bind mount via Docker)
        // We'll use bind mount instead of copying — more efficient for local dev

        // Build Docker command
        let mut cmd = Command::new("docker");
        cmd.arg("run").arg("--rm");

        // Network isolation
        if !transform.network {
            cmd.args(["--network", "none"]);
        }

        // Bind mounts
        cmd.args(["-v", &format!("{}:/workspace:rw", workspace_path.display())]);

        // Bind mount source directory if needed
        if let Some(src_dir) = source_dir {
            cmd.args(["-v", &format!("{}:/workspace/source:ro", src_dir.display())]);
        }

        // Determinism env vars
        cmd.args(["-e", "PYTHONHASHSEED=0"]);
        cmd.args(["-e", "OMP_NUM_THREADS=1"]);
        cmd.args(["-e", "MKL_NUM_THREADS=1"]);
        cmd.args(["-e", "OPENBLAS_NUM_THREADS=1"]);
        cmd.args(["-e", "NUMEXPR_NUM_THREADS=1"]);
        cmd.args(["-e", "VECLIB_MAXIMUM_THREADS=1"]);

        // User env vars
        for (key, value) in &env_vars {
            cmd.args(["-e", &format!("{}={}", key, value)]);
        }

        // Image and entrypoint
        cmd.arg(&image);
        cmd.args(["/bin/sh", "/workspace/init.sh"]);

        // Execute with timeout (5 minutes default)
        let output_result = tokio::time::timeout(std::time::Duration::from_secs(300), cmd.output())
            .await
            .map_err(|_| anyhow::anyhow!("Transform '{}' timed out after 300s", node_name))?
            .context("Failed to execute docker run. Is Docker installed and running?")?;

        let duration = start.elapsed();
        let stdout = String::from_utf8_lossy(&output_result.stdout);
        let stderr = String::from_utf8_lossy(&output_result.stderr);

        if !output_result.status.success() {
            let exit_code = output_result.status.code().unwrap_or(-1);
            eprintln!("    stdout: {}", stdout);
            eprintln!("    stderr: {}", stderr);
            bail!(
                "Transform '{}' failed with exit code {} (took {:.1}s)",
                node_name,
                exit_code,
                duration.as_secs_f64()
            );
        }

        eprintln!("    completed in {:.1}s", duration.as_secs_f64());

        // Cache output: copy workspace output to cache directory
        std::fs::create_dir_all(&cache_path)?;
        copy_dir_sync(&ws_output, &cache_path)?;

        let primary = find_primary_output(&cache_path)?;
        let output_hash = hash::blake3_hash_file(&primary)?;
        node_outputs.insert(
            node_name.clone(),
            NodeOutput {
                hash: mat_hash,
                output_hash,
                output_dir: cache_path,
            },
        );
    }

    // 11. Write final output (from the last node)
    let final_node = execution_order.last().unwrap();
    let final_output = &node_outputs[final_node];
    write_final_output(&final_output.output_dir, output)?;

    Ok(())
}

// ============================================================================
// Param resolution
// ============================================================================

/// Parse `--param key=value` args into a map of string values.
///
/// Values are kept as strings and coerced to declared types later in
/// `resolve_endpoint_params`, matching the server's query-param flow.
fn parse_param_args(params: &[String]) -> Result<HashMap<String, String>> {
    let mut map = HashMap::new();
    for param in params {
        let (key, value) = param
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("Invalid param '{}'. Expected key=value", param))?;
        map.insert(key.to_string(), value.to_string());
    }
    Ok(map)
}

/// Coerce a string param value to the declared JSON type.
///
/// Matches the server's `coerce_param_value` logic so CLI and server produce
/// identical materialized hashes for the same inputs.
fn coerce_param_value(s: &str, declared_type: &str) -> serde_json::Value {
    match declared_type {
        "float" | "number" => {
            if let Ok(n) = s.parse::<f64>() {
                return serde_json::Value::from(n);
            }
        }
        "int" | "integer" => {
            if let Ok(n) = s.parse::<i64>() {
                return serde_json::Value::from(n);
            }
        }
        "bool" | "boolean" => match s {
            "true" | "1" | "yes" => return serde_json::Value::Bool(true),
            "false" | "0" | "no" => return serde_json::Value::Bool(false),
            _ => {}
        },
        _ => {} // "string" and unknown types stay as strings
    }
    serde_json::Value::String(s.to_string())
}

/// Parse `--local-data name=path` args into a map.
fn parse_local_data_args(args: &[String]) -> Result<HashMap<String, PathBuf>> {
    let mut map = HashMap::new();
    for arg in args {
        let (name, path) = arg
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("Invalid --local-data '{}'. Expected name=path", arg))?;
        let path = PathBuf::from(path);
        if !path.exists() {
            bail!("Local data file '{}' does not exist", path.display());
        }
        map.insert(name.to_string(), path);
    }
    Ok(map)
}

/// Resolve endpoint params: apply user overrides, then fill defaults.
fn resolve_endpoint_params(
    endpoint: &EndpointDef,
    user_params: &HashMap<String, String>,
) -> Result<HashMap<String, serde_json::Value>> {
    // Check for unrecognized params
    for key in user_params.keys() {
        if !endpoint.params.contains_key(key) {
            let available: Vec<&str> = endpoint.params.keys().map(|s| s.as_str()).collect();
            bail!("Unknown parameter '{}'. Available: {:?}", key, available);
        }
    }

    let mut resolved = HashMap::new();
    for (name, def) in &endpoint.params {
        if let Some(value) = user_params.get(name) {
            // Coerce string value to declared type (matches server's coerce_param_value)
            let coerced = coerce_param_value(value, &def.type_);
            resolved.insert(name.clone(), coerced);
        } else if let Some(default) = &def.default {
            resolved.insert(name.clone(), default.clone());
        } else {
            bail!("Required param '{}' not provided and has no default", name);
        }
    }
    Ok(resolved)
}

/// Resolve params for a specific node: merge static node params with endpoint param binds.
fn resolve_node_params(
    node_name: &str,
    static_params: &HashMap<String, serde_json::Value>,
    endpoint_params: &HashMap<String, serde_json::Value>,
    endpoint: &EndpointDef,
) -> serde_json::Value {
    let mut params = serde_json::Map::new();

    // Start with static params from node definition
    for (key, value) in static_params {
        params.insert(key.clone(), value.clone());
    }

    // Override with endpoint param binds
    for (_ep_param_name, ep_param_def) in &endpoint.params {
        // binds format: "node_name.param_name"
        if let Some((bind_node, bind_param)) = ep_param_def.binds.split_once('.') {
            if bind_node == node_name {
                // Find the endpoint-level param value
                if let Some(value) = endpoint_params.get(_ep_param_name) {
                    params.insert(bind_param.to_string(), value.clone());
                }
            }
        }
    }

    serde_json::Value::Object(params)
}

// ============================================================================
// DAG execution order
// ============================================================================

/// Topological sort of endpoint nodes using Kahn's algorithm.
fn build_execution_order(endpoint: &EndpointDef) -> Result<Vec<String>> {
    let mut in_degree: HashMap<String, usize> = HashMap::new();
    let mut successors: HashMap<String, Vec<String>> = HashMap::new();

    for node_name in endpoint.nodes.keys() {
        in_degree.entry(node_name.clone()).or_insert(0);
        successors.entry(node_name.clone()).or_default();
    }

    for edge in &endpoint.edges {
        let source = parse_edge_source(&edge.from);
        if let EdgeSource::Node(src_node) = source {
            if let Some((tgt_node, _)) = parse_edge_target(&edge.to) {
                if endpoint.nodes.contains_key(&src_node) && endpoint.nodes.contains_key(&tgt_node)
                {
                    successors
                        .entry(src_node)
                        .or_default()
                        .push(tgt_node.clone());
                    *in_degree.entry(tgt_node).or_insert(0) += 1;
                }
            }
        }
    }

    let mut queue: VecDeque<String> = in_degree
        .iter()
        .filter(|(_, deg)| **deg == 0)
        .map(|(name, _)| name.clone())
        .collect();

    // Sort the initial queue for deterministic ordering
    let mut initial: Vec<String> = queue.drain(..).collect();
    initial.sort();
    queue.extend(initial);

    let mut order = Vec::new();
    while let Some(node) = queue.pop_front() {
        order.push(node.clone());
        if let Some(succs) = successors.get(&node) {
            let mut ready = Vec::new();
            for s in succs {
                if let Some(deg) = in_degree.get_mut(s) {
                    *deg -= 1;
                    if *deg == 0 {
                        ready.push(s.clone());
                    }
                }
            }
            ready.sort();
            queue.extend(ready);
        }
    }

    if order.len() != endpoint.nodes.len() {
        bail!("Endpoint DAG contains a cycle");
    }

    Ok(order)
}

/// Build a map of each node's inputs to their edge sources.
fn build_edge_map(endpoint: &EndpointDef) -> HashMap<String, Vec<(String, EdgeSource)>> {
    let mut map: HashMap<String, Vec<(String, EdgeSource)>> = HashMap::new();
    for edge in &endpoint.edges {
        let source = parse_edge_source(&edge.from);
        if let Some((tgt_node, input_name)) = parse_edge_target(&edge.to) {
            map.entry(tgt_node).or_default().push((input_name, source));
        }
    }
    map
}

// ============================================================================
// Hash computation
// ============================================================================

/// Hash the transform source from local filesystem.
fn compute_source_hash(cwd: &Path, transform: &TransformDef) -> Result<String> {
    if let Some(source) = &transform.source {
        // source format: "path/to/file.py:function_name"
        let (file_path, _) = source.rsplit_once(':').ok_or_else(|| {
            anyhow::anyhow!("Invalid source '{}'. Expected path:function", source)
        })?;
        let full_path = cwd.join(file_path);
        hash::blake3_hash_file(&full_path)
            .with_context(|| format!("Cannot read transform source '{}'", full_path.display()))
    } else if let Some(command) = &transform.command {
        // For command transforms, hash the command string itself
        Ok(hash::blake3_hash(command.as_bytes()))
    } else {
        bail!("Transform has neither source nor command");
    }
}

/// Compute the environment hash from local filesystem.
fn compute_env_hash(cwd: &Path, env_def: &EnvironmentDef) -> Result<String> {
    match env_def.tier() {
        Some(EnvironmentTier::BaseLockfile { base, lockfile }) => {
            let lockfile_path = cwd.join(&lockfile);
            let lockfile_content = std::fs::read_to_string(&lockfile_path)
                .with_context(|| format!("Cannot read lockfile '{}'", lockfile_path.display()))?;
            Ok(hash::blake3_hash_components(&[
                base.as_str(),
                lockfile_content.as_str(),
            ]))
        }
        Some(EnvironmentTier::Dockerfile { dockerfile }) => {
            let df_path = cwd.join(&dockerfile);
            let df_content = std::fs::read_to_string(&df_path)
                .with_context(|| format!("Cannot read Dockerfile '{}'", df_path.display()))?;
            Ok(hash::blake3_hash(df_content.as_bytes()))
        }
        Some(EnvironmentTier::Prebuilt { image }) => {
            // Use image reference as hash
            Ok(hash::blake3_hash(image.as_bytes()))
        }
        None => bail!("Invalid environment definition"),
    }
}

/// Compute lockfile hash (for transform hash computation).
fn compute_lockfile_hash(cwd: &Path, env_def: &EnvironmentDef) -> String {
    if let Some(lockfile) = &env_def.lockfile {
        let lockfile_path = cwd.join(lockfile);
        hash::blake3_hash_file(&lockfile_path).unwrap_or_else(|_| {
            // If lockfile can't be read, use empty hash
            hash::blake3_hash(b"")
        })
    } else {
        hash::blake3_hash(b"")
    }
}

/// Compute params schema hash from transform param definitions.
fn compute_params_schema_hash(transform: &TransformDef) -> String {
    if transform.params.is_empty() {
        return hash::blake3_hash(b"");
    }
    let mut sorted_params: Vec<_> = transform.params.iter().collect();
    sorted_params.sort_by_key(|(name, _)| name.as_str());
    let schema_str: String = sorted_params
        .iter()
        .map(|(name, def)| format!("{}:{}", name, def.type_))
        .collect::<Vec<_>>()
        .join("\0");
    hash::blake3_hash(schema_str.as_bytes())
}

/// Resolve input hashes for a node (for materialized hash computation).
fn resolve_input_hashes(
    node_name: &str,
    edge_map: &HashMap<String, Vec<(String, EdgeSource)>>,
    local_data_map: &HashMap<String, PathBuf>,
    node_outputs: &HashMap<String, NodeOutput>,
) -> Result<Vec<(String, String)>> {
    let mut hashes = Vec::new();
    if let Some(edges) = edge_map.get(node_name) {
        for (input_name, source) in edges {
            let input_hash = match source {
                EdgeSource::Data(name) => {
                    if let Some(path) = local_data_map.get(name.as_str()) {
                        hash::blake3_hash_file(path).with_context(|| {
                            format!("Cannot hash local data '{}'", path.display())
                        })?
                    } else {
                        bail!(
                            "Data '{}' not provided. Use --local-data {}=<path>",
                            name,
                            name
                        );
                    }
                }
                EdgeSource::Collection(name) => {
                    bail!(
                        "Collection '{}' references are not yet supported in local execution. \
                         Use `ozzy fetch` for remote execution instead.",
                        name
                    );
                }
                EdgeSource::Endpoint(ref_str) => {
                    bail!(
                        "Endpoint reference '{}' not yet supported in local execution. \
                         Use `ozzy fetch` for remote execution instead.",
                        ref_str
                    );
                }
                EdgeSource::Node(src_node) => node_outputs
                    .get(src_node.as_str())
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "Node '{}' output not available (should have been computed earlier)",
                            src_node
                        )
                    })?
                    .output_hash
                    .clone(),
            };
            hashes.push((input_name.clone(), input_hash));
        }
    }
    Ok(hashes)
}

// ============================================================================
// Input file resolution
// ============================================================================

/// Resolve input files (actual paths) for Docker mounting.
fn resolve_input_files(
    node_name: &str,
    transform: &TransformDef,
    edge_map: &HashMap<String, Vec<(String, EdgeSource)>>,
    local_data_map: &HashMap<String, PathBuf>,
    node_outputs: &HashMap<String, NodeOutput>,
) -> Result<Vec<InputFile>> {
    let mut files = Vec::new();
    if let Some(edges) = edge_map.get(node_name) {
        for (input_name, source) in edges {
            let content_type = transform
                .inputs
                .get(input_name)
                .cloned()
                .unwrap_or_else(|| "application/octet-stream".to_string());

            let (local_path, is_collection) = match source {
                EdgeSource::Data(name) => {
                    let path = local_data_map
                        .get(name.as_str())
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                                "Data '{}' not provided. Use --local-data {}=<path>",
                                name,
                                name
                            )
                        })?
                        .clone();
                    (path, false)
                }
                EdgeSource::Node(src_node) => {
                    let output = node_outputs.get(src_node.as_str()).ok_or_else(|| {
                        anyhow::anyhow!("Node '{}' output not available", src_node)
                    })?;
                    // Find the primary output file in the output directory
                    let primary = find_primary_output(&output.output_dir)?;
                    (primary, false)
                }
                EdgeSource::Collection(_) | EdgeSource::Endpoint(_) => {
                    bail!("Collection/endpoint references not yet supported in local execution");
                }
            };

            files.push(InputFile {
                name: input_name.clone(),
                local_path,
                content_type,
                is_collection,
            });
        }
    }
    Ok(files)
}

/// Find the primary output file in a node's output directory.
fn find_primary_output(output_dir: &Path) -> Result<PathBuf> {
    let mut entries: Vec<_> = std::fs::read_dir(output_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .collect();

    if entries.is_empty() {
        bail!("No output files in {}", output_dir.display());
    }

    // Sort for deterministic selection (matches server's list_output_files)
    entries.sort_by_key(|e| e.file_name());

    // Prefer "result.*" files
    for entry in &entries {
        let name = entry.file_name();
        let name_str = name.to_str().unwrap_or("");
        if name_str.starts_with("result") {
            return Ok(entry.path());
        }
    }

    // Fall back to first file alphabetically
    Ok(entries[0].path())
}

// ============================================================================
// Environment resolution
// ============================================================================

/// Resolve (and optionally build) the Docker image for local execution.
async fn resolve_local_environment(
    cwd: &Path,
    env_def: &EnvironmentDef,
    env_hash: &str,
) -> Result<String> {
    match env_def.tier() {
        Some(EnvironmentTier::Prebuilt { image }) => {
            // Use the image directly — Docker will pull if needed
            Ok(image)
        }
        Some(EnvironmentTier::BaseLockfile { base, lockfile }) => {
            let tag = format!("ozzydb-env:{}", env_hash.get(..16).unwrap_or(env_hash));

            // Check if image already exists locally
            let check = Command::new("docker")
                .args(["image", "inspect", &tag])
                .output()
                .await;

            if let Ok(output) = check {
                if output.status.success() {
                    return Ok(tag);
                }
            }

            // Build the environment
            eprintln!("    Building environment ({})...", &tag);
            let lockfile_path = cwd.join(&lockfile);
            let lockfile_content = std::fs::read_to_string(&lockfile_path)?;

            // Note: uv.lock is TOML-based and cannot be pip-installed directly.
            // Users should provide requirements.txt (via `uv export`).
            let install_cmd = if lockfile == "poetry.lock" || lockfile.ends_with("/poetry.lock") {
                "pip install poetry && cd /tmp && poetry install --no-interaction --no-ansi"
            } else {
                // requirements.txt or any pip-compatible lockfile
                "pip install --no-cache-dir -r /tmp/lockfile"
            };

            let dockerfile = format!(
                "FROM {}\nCOPY lockfile /tmp/lockfile\nRUN {}\n",
                base, install_cmd
            );

            // Write temp Dockerfile and lockfile
            let build_dir = tempfile::tempdir()?;
            std::fs::write(build_dir.path().join("Dockerfile"), &dockerfile)?;
            std::fs::write(build_dir.path().join("lockfile"), &lockfile_content)?;

            let output = Command::new("docker")
                .args(["build", "-t", &tag, "."])
                .current_dir(build_dir.path())
                .output()
                .await
                .context("Failed to run docker build")?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                bail!("Environment build failed:\n{}", stderr);
            }

            Ok(tag)
        }
        Some(EnvironmentTier::Dockerfile { dockerfile }) => {
            let tag = format!("ozzydb-env:{}", env_hash.get(..16).unwrap_or(env_hash));

            // Check if already built
            let check = Command::new("docker")
                .args(["image", "inspect", &tag])
                .output()
                .await;

            if let Ok(output) = check {
                if output.status.success() {
                    return Ok(tag);
                }
            }

            eprintln!("    Building environment from Dockerfile ({})...", &tag);
            let output = Command::new("docker")
                .args(["build", "-t", &tag, "-f", &dockerfile, "."])
                .current_dir(cwd)
                .output()
                .await
                .context("Failed to run docker build")?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                bail!("Environment build failed:\n{}", stderr);
            }

            Ok(tag)
        }
        None => bail!("Invalid environment definition"),
    }
}

// ============================================================================
// Runner generation (inline — same templates as server)
// ============================================================================

/// Generate the runner script content and file extension.
fn generate_runner(transform: &TransformDef) -> Result<(String, String)> {
    if let Some(source) = &transform.source {
        let (file_path, function_name) = source
            .rsplit_once(':')
            .ok_or_else(|| anyhow::anyhow!("Invalid source ref '{}'", source))?;

        if file_path.ends_with(".py") {
            Ok((
                generate_python_runner(file_path, function_name),
                "py".to_string(),
            ))
        } else if file_path.ends_with(".R") || file_path.ends_with(".r") {
            // R runner — matches server's runners/r.rs template
            let script = format!(
                r#"#!/usr/bin/env Rscript
# OzzyDB R runner. Auto-generated — do not edit.
library(jsonlite)
library(arrow)

params <- fromJSON(Sys.getenv("OZZY_PARAMS", "{{}}"))
input_manifest <- fromJSON(Sys.getenv("OZZY_INPUT_MANIFEST", "{{}}"))
inputs <- list()

for (name in names(input_manifest)) {{
  spec <- input_manifest[[name]]
  if (grepl("parquet", spec$content_type)) {{
    inputs[[name]] <- read_parquet(spec$path)
  }} else if (spec$content_type == "text/csv") {{
    inputs[[name]] <- read.csv(spec$path)
  }} else if (startsWith(spec$content_type, "text/")) {{
    inputs[[name]] <- readLines(spec$path, warn = FALSE)
  }} else {{
    inputs[[name]] <- readBin(spec$path, "raw", file.info(spec$path)$size)
  }}
}}

source("/workspace/source/{source_file}")
result <- {function_name}(inputs, params)

output_dir <- "/workspace/output"
dir.create(output_dir, showWarnings = FALSE, recursive = TRUE)

if (inherits(result, "data.frame") || inherits(result, "ArrowTabular")) {{
  write_parquet(result, file.path(output_dir, "result.parquet"))
}} else if (is.character(result)) {{
  writeLines(result, file.path(output_dir, "result.txt"))
}} else if (is.raw(result)) {{
  writeBin(result, file.path(output_dir, "result.bin"))
}} else {{
  saveRDS(result, file.path(output_dir, "result.rds"))
}}
"#,
                source_file = file_path,
                function_name = function_name,
            );
            Ok((script, "R".to_string()))
        } else {
            bail!("Unsupported source file type: {}", file_path);
        }
    } else if let Some(command) = &transform.command {
        // Command-based transform: shell template substitution
        let input_names: Vec<&str> = transform.inputs.keys().map(|s| s.as_str()).collect();
        let substituted = substitute_command(command, &input_names);
        let wrapper = format!(
            "#!/bin/sh\nset -e\nmkdir -p /workspace/output\n{}\n",
            substituted
        );
        Ok((wrapper, "sh".to_string()))
    } else {
        bail!("Transform has neither source nor command");
    }
}

/// Generate the Python runner script (same template as server's runners/python.rs).
fn generate_python_runner(source_file: &str, function_name: &str) -> String {
    let module = source_file
        .strip_suffix(".py")
        .unwrap_or(source_file)
        .replace('/', ".");

    format!(
        r#"#!/usr/bin/env python3
"""OzzyDB Python runner. Auto-generated — do not edit."""
import sys
import os
import json

sys.path.insert(0, '/workspace/source')

params = json.loads(os.environ.get("OZZY_PARAMS", "{{}}"))
input_manifest = json.loads(os.environ.get("OZZY_INPUT_MANIFEST", "{{}}"))


def _load_item(path, content_type):
    if "parquet" in content_type:
        import polars as pl
        return pl.read_parquet(path)
    elif content_type.startswith("image/"):
        with open(path, "rb") as f:
            return f.read()
    elif content_type == "application/json":
        with open(path) as f:
            return json.loads(f.read())
    elif content_type.startswith("text/"):
        with open(path) as f:
            return f.read()
    else:
        with open(path, "rb") as f:
            return f.read()


inputs = {{}}
for name, spec in input_manifest.items():
    path = spec["path"]
    content_type = spec["content_type"]
    is_collection = spec.get("is_collection", False)

    if is_collection:
        with open(spec["manifest_path"]) as f:
            member_manifest = json.loads(f.read())
        members = []
        for member in member_manifest:
            members.append(_load_item(member["path"], member["content_type"]))
        inputs[name] = members
    else:
        inputs[name] = _load_item(path, content_type)


def _write_item(item, path):
    if hasattr(item, 'collect'):
        item = item.collect()
    if hasattr(item, 'write_parquet'):
        item.write_parquet(path + ".parquet")
    elif isinstance(item, (bytes, bytearray)):
        with open(path, "wb") as f:
            f.write(item)
    elif isinstance(item, str):
        with open(path, "w") as f:
            f.write(item)
    elif isinstance(item, dict):
        with open(path + ".json", "w") as f:
            json.dump(item, f)
    else:
        raise TypeError(f"Unsupported output type: {{type(item)}}")


from {module} import {function_name}

result = {function_name}(inputs, params)

output_dir = "/workspace/output"
os.makedirs(output_dir, exist_ok=True)

if isinstance(result, list):
    manifest = []
    for i, item in enumerate(result):
        out_path = os.path.join(output_dir, f"item_{{i:06d}}")
        _write_item(item, out_path)
        manifest.append({{"index": i, "path": out_path}})
    with open(os.path.join(output_dir, "manifest.json"), "w") as f:
        json.dump(manifest, f)
else:
    out_path = os.path.join(output_dir, "result")
    _write_item(result, out_path)
"#,
        module = module,
        function_name = function_name,
    )
}

/// Substitute command template variables.
fn substitute_command(command: &str, input_names: &[&str]) -> String {
    let mut result = command.to_string();
    for name in input_names {
        let pattern = format!("${{input.{}}}", name);
        let replacement = format!("/workspace/inputs/{}", name);
        result = result.replace(&pattern, &replacement);
    }
    result = result.replace("${output}", "/workspace/output/result");
    result
}

/// Generate the init script.
fn generate_init_script(transform: &TransformDef) -> String {
    let runner_cmd = if transform.source.is_some() {
        let source = transform.source.as_deref().unwrap();
        let (file_path, _) = source.rsplit_once(':').unwrap_or((source, ""));
        if file_path.ends_with(".py") {
            "python3 /workspace/runner.py"
        } else if file_path.ends_with(".R") || file_path.ends_with(".r") {
            "Rscript /workspace/runner.R"
        } else {
            "python3 /workspace/runner.py"
        }
    } else {
        "/bin/sh /workspace/runner.sh"
    };

    format!(
        r#"#!/bin/sh
set -e
echo "OzzyDB init: starting transform execution"
mkdir -p /workspace/output
{runner_cmd}
if [ -z "$(ls -A /workspace/output 2>/dev/null)" ]; then
    echo "ERROR: Transform produced no output in /workspace/output/" >&2
    exit 1
fi
echo "OzzyDB init: transform completed successfully"
"#,
        runner_cmd = runner_cmd,
    )
}

// ============================================================================
// Input manifest (same format as server's compute/docker.rs)
// ============================================================================

fn build_input_manifest(inputs: &[InputFile]) -> serde_json::Value {
    let mut manifest = serde_json::Map::new();
    for input in inputs {
        let container_path = format!("/workspace/inputs/{}", input.name);
        let mut spec = serde_json::Map::new();
        spec.insert("path".to_string(), serde_json::json!(container_path));
        spec.insert(
            "content_type".to_string(),
            serde_json::json!(input.content_type),
        );
        spec.insert(
            "is_collection".to_string(),
            serde_json::json!(input.is_collection),
        );
        if input.is_collection {
            spec.insert(
                "manifest_path".to_string(),
                serde_json::json!(format!("{}/manifest.json", container_path)),
            );
        }
        manifest.insert(input.name.clone(), serde_json::Value::Object(spec));
    }
    serde_json::Value::Object(manifest)
}

// ============================================================================
// Output handling
// ============================================================================

/// Write the final endpoint output to stdout or a file.
fn write_final_output(output_dir: &Path, output_path: Option<&str>) -> Result<()> {
    let primary = find_primary_output(output_dir)?;
    let bytes = std::fs::read(&primary).context("Failed to read output file")?;

    if let Some(path) = output_path {
        std::fs::write(path, &bytes).context("Failed to write output file")?;
        eprintln!("Wrote {} bytes to {}", bytes.len(), path);
    } else {
        use std::io::Write;
        std::io::stdout().write_all(&bytes)?;
    }

    Ok(())
}

// ============================================================================
// Filesystem utilities
// ============================================================================

/// Recursively copy a directory (sync), skipping symlinks.
fn copy_dir_sync(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        // Skip symlinks to prevent data exfiltration from sandbox
        if ft.is_symlink() {
            eprintln!("Warning: skipping symlink {}", entry.path().display());
            continue;
        }
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if ft.is_dir() {
            copy_dir_sync(&src_path, &dst_path)?;
        } else if ft.is_file() {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use ozzy_core::toml_spec::OzzyToml;

    fn test_spec() -> OzzyToml {
        OzzyToml::parse(
            r#"
[project]
name = "test"
owner = "user"

[environments.default]
base = "python:3.12"
lockfile = "requirements.txt"

[transforms.clean]
source = "transforms/clean.py:clean_fn"
environment = "default"
inputs.readings = "parquet"
output = "parquet"

[transforms.cal]
source = "transforms/cal.py:calibrate"
environment = "default"
inputs.data = "parquet"
output = "parquet"

[transforms.cal.params.method]
type = "string"

[endpoints.corrected]
description = "QC'd data"

[endpoints.corrected.params.cal_method]
type = "string"
default = "leff_2024"
binds = "cal.method"

[endpoints.corrected.nodes]
clean = { transform = "clean" }
cal = { transform = "cal" }

[[endpoints.corrected.edges]]
from = "data:raw_readings"
to = "clean.readings"

[[endpoints.corrected.edges]]
from = "clean"
to = "cal.data"
"#,
        )
        .unwrap()
    }

    #[test]
    fn test_build_execution_order() {
        let spec = test_spec();
        let ep = &spec.endpoints["corrected"];
        let order = build_execution_order(ep).unwrap();
        assert_eq!(order, vec!["clean", "cal"]);
    }

    #[test]
    fn test_build_edge_map() {
        let spec = test_spec();
        let ep = &spec.endpoints["corrected"];
        let map = build_edge_map(ep);

        let clean_edges = &map["clean"];
        assert_eq!(clean_edges.len(), 1);
        assert_eq!(clean_edges[0].0, "readings");
        assert!(matches!(&clean_edges[0].1, EdgeSource::Data(n) if n == "raw_readings"));

        let cal_edges = &map["cal"];
        assert_eq!(cal_edges.len(), 1);
        assert_eq!(cal_edges[0].0, "data");
        assert!(matches!(&cal_edges[0].1, EdgeSource::Node(n) if n == "clean"));
    }

    #[test]
    fn test_resolve_endpoint_params_with_defaults() {
        let spec = test_spec();
        let ep = &spec.endpoints["corrected"];
        let user_params: HashMap<String, String> = HashMap::new();
        let resolved = resolve_endpoint_params(ep, &user_params).unwrap();
        assert_eq!(
            resolved.get("cal_method"),
            Some(&serde_json::json!("leff_2024"))
        );
    }

    #[test]
    fn test_resolve_endpoint_params_with_override() {
        let spec = test_spec();
        let ep = &spec.endpoints["corrected"];
        let mut user_params = HashMap::new();
        user_params.insert("cal_method".to_string(), "smith_2023".to_string());
        let resolved = resolve_endpoint_params(ep, &user_params).unwrap();
        assert_eq!(
            resolved.get("cal_method"),
            Some(&serde_json::json!("smith_2023"))
        );
    }

    #[test]
    fn test_resolve_node_params() {
        let spec = test_spec();
        let ep = &spec.endpoints["corrected"];
        let mut ep_params = HashMap::new();
        ep_params.insert("cal_method".to_string(), serde_json::json!("leff_2024"));

        let result = resolve_node_params("cal", &HashMap::new(), &ep_params, ep);
        assert_eq!(result.get("method"), Some(&serde_json::json!("leff_2024")));
    }

    #[test]
    fn test_coerce_param_value() {
        // Float coercion
        assert_eq!(coerce_param_value("3.14", "float"), serde_json::json!(3.14));
        assert_eq!(coerce_param_value("42", "number"), serde_json::json!(42.0));
        // Integer coercion
        assert_eq!(coerce_param_value("42", "int"), serde_json::json!(42));
        assert_eq!(coerce_param_value("42", "integer"), serde_json::json!(42));
        // Boolean coercion (matches server: true/1/yes, false/0/no)
        assert_eq!(coerce_param_value("true", "bool"), serde_json::json!(true));
        assert_eq!(coerce_param_value("1", "bool"), serde_json::json!(true));
        assert_eq!(coerce_param_value("yes", "boolean"), serde_json::json!(true));
        assert_eq!(coerce_param_value("false", "bool"), serde_json::json!(false));
        assert_eq!(coerce_param_value("0", "bool"), serde_json::json!(false));
        assert_eq!(coerce_param_value("no", "boolean"), serde_json::json!(false));
        // String stays as string
        assert_eq!(coerce_param_value("hello", "string"), serde_json::json!("hello"));
        // Numeric-looking value stays string when declared as string
        assert_eq!(coerce_param_value("123", "string"), serde_json::json!("123"));
        assert_eq!(coerce_param_value("true", "string"), serde_json::json!("true"));
        // Failed coercion falls back to string
        assert_eq!(coerce_param_value("not-a-number", "float"), serde_json::json!("not-a-number"));
    }

    #[test]
    fn test_parse_param_args() {
        let args = vec!["threshold=12.5".to_string(), "method=voltage".to_string()];
        let parsed = parse_param_args(&args).unwrap();
        assert_eq!(parsed.get("threshold"), Some(&"12.5".to_string()));
        assert_eq!(parsed.get("method"), Some(&"voltage".to_string()));
    }

    #[test]
    fn test_generate_python_runner() {
        let script = generate_python_runner("transforms/qc.py", "quality_control");
        assert!(script.contains("from transforms.qc import quality_control"));
        assert!(script.contains("result = quality_control(inputs, params)"));
    }

    #[test]
    fn test_substitute_command() {
        let result = substitute_command("ffmpeg -i ${input.video} -o ${output}", &["video"]);
        assert_eq!(
            result,
            "ffmpeg -i /workspace/inputs/video -o /workspace/output/result"
        );
    }

    #[test]
    fn test_build_input_manifest() {
        let inputs = vec![InputFile {
            name: "readings".to_string(),
            local_path: PathBuf::from("/data/readings.parquet"),
            content_type: "application/vnd.apache.parquet".to_string(),
            is_collection: false,
        }];
        let manifest = build_input_manifest(&inputs);
        let readings = manifest.get("readings").unwrap();
        assert_eq!(readings.get("path").unwrap(), "/workspace/inputs/readings");
        assert_eq!(readings.get("is_collection").unwrap(), false);
    }

    #[test]
    fn test_compute_params_schema_hash_stable() {
        let spec = test_spec();
        let t = &spec.transforms["cal"];
        let h1 = compute_params_schema_hash(t);
        let h2 = compute_params_schema_hash(t);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn test_compute_params_schema_hash_empty() {
        let spec = test_spec();
        let t = &spec.transforms["clean"];
        let h = compute_params_schema_hash(t);
        assert_eq!(h.len(), 64);
    }
}
