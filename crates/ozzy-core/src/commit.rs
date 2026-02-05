//! Commit operations - creating and managing commits.

use chrono::Utc;
use std::collections::HashMap;
use std::fs;

use crate::canon::hash_source_file;
use crate::error::Result;
use crate::hash::{blake3_hash, blake3_hash_file};
use crate::project::{Commit, DataSource, Project, Transform};
use crate::schema::extract_parquet_schema;

/// Build a commit from the current workspace state.
pub fn create_commit(project: &Project, message: &str, author: &str) -> Result<Commit> {
    // Collect data sources from data/ directory
    let data_sources = collect_data_sources(project)?;

    // Collect transforms from transforms/ directory
    let transforms = collect_transforms(project)?;

    // Get endpoints from the last commit (they're modified via ozzy endpoint commands)
    let endpoints = if let Some(last_commit) = project.latest_commit()? {
        last_commit.endpoints
    } else {
        HashMap::new()
    };

    // Get parent hashes
    let parent_hashes = if let Some(head) = project.head_commit()? {
        vec![head]
    } else {
        vec![]
    };

    // Build the commit hash from all content
    let commit_content = serde_json::json!({
        "parent_hashes": parent_hashes,
        "data_sources": data_sources,
        "transforms": transforms,
        "endpoints": endpoints,
        "author": author,
        "message": message,
    });
    let hash = blake3_hash(commit_content.to_string().as_bytes());

    Ok(Commit {
        hash,
        parent_hashes,
        author: author.to_string(),
        message: message.to_string(),
        timestamp: Utc::now(),
        data_sources,
        transforms,
        endpoints,
    })
}

/// Collect data sources from the data/ directory.
pub fn collect_data_sources(project: &Project) -> Result<HashMap<String, DataSource>> {
    let mut sources = HashMap::new();
    let data_dir = project.data_dir();

    if !data_dir.exists() {
        return Ok(sources);
    }

    for entry in fs::read_dir(&data_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().map(|e| e == "parquet").unwrap_or(false) {
            let name = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();

            let hash = blake3_hash_file(&path)?;
            let schema = extract_parquet_schema(&path)?;
            let schema_hash = blake3_hash(serde_json::to_string(&schema)?.as_bytes());

            let metadata = fs::metadata(&path)?;

            sources.insert(
                name.clone(),
                DataSource {
                    name,
                    hash,
                    schema_hash,
                    path: format!("data/{}", path.file_name().unwrap().to_string_lossy()),
                    row_count: None, // TODO: read from parquet metadata
                    byte_size: Some(metadata.len()),
                },
            );
        }
    }

    Ok(sources)
}

/// Collect transforms from the transforms/ directory.
pub fn collect_transforms(project: &Project) -> Result<HashMap<String, Transform>> {
    let mut transforms = HashMap::new();
    let transforms_dir = project.transforms_dir();

    if !transforms_dir.exists() {
        return Ok(transforms);
    }

    for entry in fs::read_dir(&transforms_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().map(|e| e == "py").unwrap_or(false) {
            // Parse Python file to find transform definitions
            let content = fs::read_to_string(&path)?;

            // Look for @ozzy.transform decorated functions
            for transform in parse_python_transforms(&content, &path)? {
                transforms.insert(transform.name.clone(), transform);
            }
        }
    }

    Ok(transforms)
}

/// Parse Python file for transform definitions.
fn parse_python_transforms(content: &str, path: &std::path::Path) -> Result<Vec<Transform>> {
    let mut transforms = Vec::new();
    let source_hash = hash_source_file(path)?;
    let relative_path = path.file_name().unwrap().to_string_lossy().to_string();

    // Simple parser: look for @ozzy.transform and the following def
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();

        if line.starts_with("@ozzy.transform") {
            // Found a decorator, look for params in the decorator
            let mut decorator_content = String::new();
            let mut j = i;

            // Collect full decorator (may span multiple lines)
            let mut paren_depth = 0;
            while j < lines.len() {
                let l = lines[j];
                decorator_content.push_str(l);
                decorator_content.push('\n');

                paren_depth += l.chars().filter(|&c| c == '(').count() as i32;
                paren_depth -= l.chars().filter(|&c| c == ')').count() as i32;

                if paren_depth == 0 && j > i {
                    break;
                }
                j += 1;
            }

            // Find the function definition
            j += 1;
            while j < lines.len() {
                let l = lines[j].trim();
                if l.starts_with("def ") {
                    // Extract function name
                    if let Some(name_end) = l.find('(') {
                        let function_name = l[4..name_end].trim().to_string();

                        // Parse decorator for metadata
                        let (params_schema, reproducible) =
                            parse_transform_decorator(&decorator_content);

                        // Look for lockfile (uv.lock in same directory)
                        let lockfile_path = path.parent().unwrap().join("uv.lock");
                        let lockfile_hash = if lockfile_path.exists() {
                            blake3_hash_file(&lockfile_path)?
                        } else {
                            // No lockfile - use empty hash
                            blake3_hash(b"")
                        };

                        transforms.push(Transform {
                            name: function_name.clone(),
                            hash: source_hash.clone(),
                            runtime: "python".to_string(),
                            source_path: format!("transforms/{}", relative_path),
                            function_name,
                            lockfile_hash,
                            params_schema,
                            reproducible,
                            input_schema: None,
                            output_schema: None,
                        });
                    }
                    break;
                }
                j += 1;
            }
            i = j;
        }
        i += 1;
    }

    Ok(transforms)
}

/// Parse transform decorator for params schema and reproducible flag.
fn parse_transform_decorator(decorator: &str) -> (serde_json::Value, bool) {
    // Simple extraction - in production, use a proper Python parser
    let mut params_schema = serde_json::json!({});
    let mut reproducible = true;

    // Check for reproducible=False
    if decorator.contains("reproducible=False") || decorator.contains("reproducible = False") {
        reproducible = false;
    }

    // Extract params if present
    // This is a simplified parser - full implementation would use Python AST
    if decorator.contains("params=") || decorator.contains("params =") {
        // For now, just mark that params exist
        params_schema = serde_json::json!({"_has_params": true});
    }

    (params_schema, reproducible)
}

/// Check if the workspace has changes compared to the last commit.
pub fn has_changes(project: &Project) -> Result<bool> {
    let current_data = collect_data_sources(project)?;
    let current_transforms = collect_transforms(project)?;

    if let Some(last_commit) = project.latest_commit()? {
        // Compare data sources
        if current_data != last_commit.data_sources {
            return Ok(true);
        }

        // Compare transforms
        for (name, transform) in &current_transforms {
            if let Some(last_transform) = last_commit.transforms.get(name) {
                if transform.hash != last_transform.hash {
                    return Ok(true);
                }
            } else {
                return Ok(true);
            }
        }

        // Check for removed transforms
        for name in last_commit.transforms.keys() {
            if !current_transforms.contains_key(name) {
                return Ok(true);
            }
        }

        Ok(false)
    } else {
        // No previous commit - there are changes if there's any data or transforms
        Ok(!current_data.is_empty() || !current_transforms.is_empty())
    }
}
