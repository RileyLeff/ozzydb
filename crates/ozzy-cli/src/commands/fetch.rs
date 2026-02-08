//! Fetch command for downloading and executing remote endpoints.

use anyhow::{Context, Result};
use ozzy_core::cache::LocalCache;
use ozzy_core::project::Endpoint;
use ozzy_core::registry::RegistryClient;
use ozzy_core::{commit, platform};
use std::io::Write;
use std::path::PathBuf;

use super::shared::{
    build_param_overrides, checked_destination, load_credentials, sanitize_archive_relative_path,
};

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

    if endpoint.is_empty() {
        anyhow::bail!("Endpoint name cannot be empty. Expected format: owner/project/endpoint[@ref]");
    }
    if ref_name.is_empty() {
        anyhow::bail!("Ref name cannot be empty. Expected format: owner/project/endpoint@ref");
    }

    Ok((registry, owner, project, endpoint, ref_name))
}

/// Get the default registry URL.
fn default_registry() -> String {
    std::env::var("OZZY_REGISTRY").unwrap_or_else(|_| "https://api.ozzydb.com".to_string())
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

    // Build expected hash maps from manifest for content verification
    let expected_data: std::collections::HashMap<String, String> = manifest
        .data_hashes
        .iter()
        .map(|(name, hash)| (format!("data/{}.parquet", name), hash.clone()))
        .collect();
    let expected_transforms: std::collections::HashMap<String, String> = manifest
        .transform_hashes
        .iter()
        .map(|(name, hash)| (format!("transforms/{}.py", name), hash.clone()))
        .collect();

    let canonical_temp_path = temp_path.canonicalize()?;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let raw_path = entry.path()?.to_path_buf();
        let path = sanitize_archive_relative_path(&raw_path)?;

        let mut content = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut content)?;

        // Verify content hashes against manifest
        let rel = path.to_string_lossy().replace('\\', "/");
        if let Some(expected) = expected_data.get(&rel) {
            let actual = ozzy_core::hash::blake3_hash(&content);
            if actual != *expected {
                anyhow::bail!(
                    "Data hash mismatch for {}: expected {}, got {}",
                    rel,
                    expected,
                    actual
                );
            }
        }
        if let Some(expected) = expected_transforms.get(&rel) {
            let text = std::str::from_utf8(&content)
                .with_context(|| format!("Transform {} is not valid UTF-8", rel))?;
            let canonical = ozzy_core::canon::canonicalize_source(text);
            let actual = ozzy_core::hash::blake3_hash(canonical.as_bytes());
            if actual != *expected {
                anyhow::bail!(
                    "Transform hash mismatch for {}: expected {}, got {}",
                    rel,
                    expected,
                    actual
                );
            }
        }

        let dest_path = checked_destination(temp_path, &canonical_temp_path, &path)?;

        let mut file = std::fs::File::create(&dest_path)?;
        file.write_all(&content)?;
    }

    // Verify all expected files from the manifest were present in the archive.
    let seen_data: std::collections::HashSet<String> = expected_data
        .keys()
        .filter(|path| temp_path.join(path).exists())
        .cloned()
        .collect();
    let seen_transforms: std::collections::HashSet<String> = expected_transforms
        .keys()
        .filter(|path| temp_path.join(path).exists())
        .cloned()
        .collect();
    for expected_path in expected_data.keys() {
        if !seen_data.contains(expected_path) {
            anyhow::bail!(
                "Incomplete archive: missing expected data file '{}'",
                expected_path
            );
        }
    }
    for expected_path in expected_transforms.keys() {
        if !seen_transforms.contains(expected_path) {
            anyhow::bail!(
                "Incomplete archive: missing expected transform file '{}'",
                expected_path
            );
        }
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
    println!();

    // Execute the pipeline
    let (execution_order, node_outputs, mut nocache_cleanup) = super::shared::execute_pipeline(
        &temp_project,
        &endpoint_def,
        &data_sources,
        &transforms,
        &global_param_overrides,
        &scoped_param_overrides,
        &plat,
        &local_cache,
        false, // fetch never uses --force
    )
    .await?;

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

#[cfg(test)]
mod tests {
    use super::super::shared::sanitize_archive_relative_path;
    use super::parse_endpoint_ref;
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

    #[test]
    fn parse_endpoint_ref_valid() {
        let (registry, owner, project, endpoint, ref_name) =
            parse_endpoint_ref("alice/myproject/clean").unwrap();
        assert!(registry.is_none());
        assert_eq!(owner, "alice");
        assert_eq!(project, "myproject");
        assert_eq!(endpoint, "clean");
        assert_eq!(ref_name, "main"); // default
    }

    #[test]
    fn parse_endpoint_ref_with_ref() {
        let (_, _, _, endpoint, ref_name) =
            parse_endpoint_ref("alice/myproject/clean@v1.0").unwrap();
        assert_eq!(endpoint, "clean");
        assert_eq!(ref_name, "v1.0");
    }

    #[test]
    fn parse_endpoint_ref_with_registry() {
        let (registry, owner, _, _, _) =
            parse_endpoint_ref("https://example.com/alice/myproject/clean").unwrap();
        assert_eq!(registry.unwrap(), "https://example.com");
        assert_eq!(owner, "alice");
    }

    #[test]
    fn parse_endpoint_ref_too_few_parts() {
        assert!(parse_endpoint_ref("alice/myproject").is_err());
    }

    #[test]
    fn parse_endpoint_ref_empty_at_ref() {
        // "alice/myproject/clean@" -> ref_name is empty
        assert!(parse_endpoint_ref("alice/myproject/clean@").is_err());
    }
}
