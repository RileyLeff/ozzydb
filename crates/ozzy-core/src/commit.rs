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
                        let meta = parse_transform_decorator(&decorator_content);

                        // Look for lockfile (uv.lock in same directory)
                        let lockfile_path = path.parent().unwrap().join("uv.lock");
                        let lockfile_hash = if lockfile_path.exists() {
                            blake3_hash_file(&lockfile_path)?
                        } else {
                            // No lockfile - use empty hash
                            blake3_hash(b"")
                        };

                        // Build input_schema with inputs list and any requires
                        let input_schema = if meta.inputs.len() > 1 || meta.input_schema.is_some() {
                            let mut schema = serde_json::Map::new();
                            if meta.inputs.len() > 1 || meta.inputs.first().map(|s| s != "main").unwrap_or(false) {
                                schema.insert("inputs".to_string(), serde_json::json!(meta.inputs));
                            }
                            if let Some(input_sch) = &meta.input_schema {
                                if let Some(obj) = input_sch.as_object() {
                                    for (k, v) in obj {
                                        schema.insert(k.clone(), v.clone());
                                    }
                                }
                            }
                            if schema.is_empty() {
                                None
                            } else {
                                Some(serde_json::Value::Object(schema))
                            }
                        } else {
                            meta.input_schema
                        };

                        transforms.push(Transform {
                            name: function_name.clone(),
                            hash: source_hash.clone(),
                            runtime: "python".to_string(),
                            source_path: format!("transforms/{}", relative_path),
                            function_name,
                            lockfile_hash,
                            params_schema: meta.params_schema,
                            reproducible: meta.reproducible,
                            input_schema,
                            output_schema: meta.output_schema,
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

/// Extracted metadata from a transform decorator.
#[derive(Debug)]
struct TransformDecoratorMeta {
    params_schema: serde_json::Value,
    reproducible: bool,
    inputs: Vec<String>,
    input_schema: Option<serde_json::Value>,
    output_schema: Option<serde_json::Value>,
}

/// Parse transform decorator for params schema, reproducible flag, and schema hints.
fn parse_transform_decorator(decorator: &str) -> TransformDecoratorMeta {
    let mut meta = TransformDecoratorMeta {
        params_schema: serde_json::json!({}),
        reproducible: true,
        inputs: vec!["main".to_string()],
        input_schema: None,
        output_schema: None,
    };

    // Check for reproducible=False
    if decorator.contains("reproducible=False") || decorator.contains("reproducible = False") {
        meta.reproducible = false;
    }

    // Extract inputs list: inputs=["main", "meta"] or inputs={"main": ..., "meta": ...}
    if let Some(inputs_start) = decorator.find("inputs=") {
        let rest = &decorator[inputs_start + 7..];
        if let Some(inputs) = extract_python_list(rest) {
            meta.inputs = inputs;
        }
    }

    // Extract params: params={"threshold": float, "prefix": str}
    if let Some(params_start) = decorator.find("params=") {
        let rest = &decorator[params_start + 7..];
        if let Some(params_dict) = extract_python_dict_raw(rest) {
            // Parse the param types
            let mut params_map = serde_json::Map::new();
            for (key, value) in parse_simple_dict(&params_dict) {
                // Convert Python type names to JSON schema-ish format
                let type_str = match value.as_str() {
                    "float" | "float64" => "number",
                    "int" | "int64" => "integer",
                    "str" | "string" => "string",
                    "bool" | "boolean" => "boolean",
                    _ => "any",
                };
                params_map.insert(key, serde_json::json!({"type": type_str}));
            }
            meta.params_schema = serde_json::Value::Object(params_map);
        }
    }

    // Extract input_schema: input_schema={"requires": ["col1", "col2"]}
    if let Some(schema_start) = decorator.find("input_schema=") {
        let rest = &decorator[schema_start + 13..];
        if let Some(dict_str) = extract_python_dict_raw(rest) {
            // Try to parse as JSON (Python dicts are close to JSON)
            let json_str = dict_str
                .replace('\'', "\"")
                .replace("True", "true")
                .replace("False", "false")
                .replace("None", "null");
            if let Ok(schema) = serde_json::from_str(&json_str) {
                meta.input_schema = Some(schema);
            } else {
                // Manual parse for requires=[...]
                if let Some(req_start) = dict_str.find("requires") {
                    let req_rest = &dict_str[req_start..];
                    if let Some(cols) = extract_python_list(req_rest) {
                        meta.input_schema = Some(serde_json::json!({"requires": cols}));
                    }
                }
            }
        }
    }

    // Extract output_schema: output_schema={"adds": ["new_col"]}
    if let Some(schema_start) = decorator.find("output_schema=") {
        let rest = &decorator[schema_start + 14..];
        if let Some(dict_str) = extract_python_dict_raw(rest) {
            let json_str = dict_str
                .replace('\'', "\"")
                .replace("True", "true")
                .replace("False", "false")
                .replace("None", "null");
            if let Ok(schema) = serde_json::from_str(&json_str) {
                meta.output_schema = Some(schema);
            } else {
                // Manual parse for adds=[...]
                if let Some(adds_start) = dict_str.find("adds") {
                    let adds_rest = &dict_str[adds_start..];
                    if let Some(cols) = extract_python_list(adds_rest) {
                        meta.output_schema = Some(serde_json::json!({"adds": cols}));
                    }
                }
            }
        }
    }

    meta
}

/// Extract a Python list like ["a", "b", "c"] from a string starting with [ or =.
fn extract_python_list(s: &str) -> Option<Vec<String>> {
    let s = s.trim();
    let start = s.find('[')?;
    let s = &s[start..];

    let mut depth = 0;
    let mut end = 0;
    for (i, c) in s.char_indices() {
        match c {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    end = i;
                    break;
                }
            }
            _ => {}
        }
    }

    if end == 0 {
        return None;
    }

    let list_content = &s[1..end];
    let items: Vec<String> = list_content
        .split(',')
        .map(|item| {
            item.trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string()
        })
        .filter(|s| !s.is_empty())
        .collect();

    if items.is_empty() {
        None
    } else {
        Some(items)
    }
}

/// Extract a Python dict like {"a": 1, "b": 2} from a string starting with {.
fn extract_python_dict_raw(s: &str) -> Option<String> {
    let s = s.trim();
    let start = s.find('{')?;
    let s = &s[start..];

    let mut depth = 0;
    let mut end = 0;
    for (i, c) in s.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = i + 1;
                    break;
                }
            }
            _ => {}
        }
    }

    if end == 0 {
        None
    } else {
        Some(s[..end].to_string())
    }
}

/// Parse a simple Python dict string into key-value pairs.
/// Handles: {"key": value, "key2": value2}
fn parse_simple_dict(s: &str) -> Vec<(String, String)> {
    let s = s.trim().trim_start_matches('{').trim_end_matches('}');
    let mut result = Vec::new();

    // Split by comma, but be careful about nested structures
    let mut items = Vec::new();
    let mut current = String::new();
    let mut depth = 0;

    for c in s.chars() {
        match c {
            '{' | '[' => {
                depth += 1;
                current.push(c);
            }
            '}' | ']' => {
                depth -= 1;
                current.push(c);
            }
            ',' if depth == 0 => {
                if !current.trim().is_empty() {
                    items.push(current.trim().to_string());
                }
                current = String::new();
            }
            _ => current.push(c),
        }
    }
    if !current.trim().is_empty() {
        items.push(current.trim().to_string());
    }

    for item in items {
        if let Some(colon_pos) = item.find(':') {
            let key = item[..colon_pos]
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string();
            let value = item[colon_pos + 1..].trim().to_string();
            result.push((key, value));
        }
    }

    result
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
