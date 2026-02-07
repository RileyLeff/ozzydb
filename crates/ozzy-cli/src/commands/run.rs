use anyhow::Result;
use ozzy_core::cache::LocalCache;
use ozzy_core::project::Endpoint;
use ozzy_core::{Project, commit, platform};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use super::shared::{self, build_param_overrides, is_nocache_output};

fn default_non_cached_output_path(endpoint_name: &str) -> PathBuf {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    PathBuf::from(format!("{}-{}.parquet", endpoint_name, ts))
}

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
    let (global_param_overrides, scoped_param_overrides) = build_param_overrides(cli_params);

    // Get platform fingerprint
    let platform = platform::PlatformFingerprint::detect();
    println!("Platform: {}", platform.short_string());

    // Open local cache
    let local_cache = LocalCache::open()?;

    if !cli_params.is_empty() {
        println!();
        println!("Parameters:");
        for (key, value) in cli_params {
            println!("  {} = {}", key, value);
        }
    }
    println!();

    // Execute the pipeline
    let (execution_order, node_outputs, mut nocache_cleanup) = shared::execute_pipeline(
        &project,
        &endpoint,
        &data_sources,
        &transforms,
        &global_param_overrides,
        &scoped_param_overrides,
        &platform,
        &local_cache,
        force,
    )
    .await?;

    // Get final output
    let final_node = execution_order
        .last()
        .ok_or_else(|| anyhow::anyhow!("Empty execution plan - endpoint has no nodes"))?;
    let final_output = node_outputs
        .get(final_node)
        .ok_or_else(|| anyhow::anyhow!("Final node '{}' output not found", final_node))?;

    let final_is_nocache = is_nocache_output(final_output);

    // Copy to output location if specified
    if let Some(output_path) = output {
        fs::copy(final_output, output_path)?;
        println!();
        println!("Output written to: {}", output_path);
    } else if final_is_nocache {
        let output_path = default_non_cached_output_path(endpoint_name);
        fs::copy(final_output, &output_path)?;
        println!();
        println!("Output written to: {}", output_path.display());
    } else {
        println!();
        println!("Output cached at: {}", final_output.display());
    }

    nocache_cleanup.cleanup()?;

    Ok(())
}

fn find_endpoint(project: &Project, name: &str) -> Result<Endpoint> {
    // Check staged endpoints first
    let staged_path = project
        .ozzy_dir()
        .join("staged_endpoints")
        .join(format!("{}.json", name));
    if staged_path.exists() {
        let content = fs::read_to_string(&staged_path)?;
        let endpoint: Endpoint = serde_json::from_str(&content)?;
        return Ok(endpoint);
    }

    // If the endpoint is staged for deletion, treat it as absent.
    let deletion_marker = project
        .ozzy_dir()
        .join("staged_endpoints")
        .join(format!("{}.deleted", name));
    if deletion_marker.exists() {
        anyhow::bail!("Endpoint '{}' is staged for deletion", name);
    }

    // Check committed endpoints
    if let Some(commit) = project.latest_commit()? {
        if let Some(endpoint) = commit.endpoints.get(name) {
            return Ok(endpoint.clone());
        }
    }

    anyhow::bail!("Endpoint '{}' not found", name);
}
