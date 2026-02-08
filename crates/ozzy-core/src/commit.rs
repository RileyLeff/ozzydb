//! Commit operations - creating and managing commits.

use chrono::Utc;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::canon;
use crate::canon::hash_source_file;
use crate::error::Result;
use crate::hash::{blake3_hash, blake3_hash_file, transform_hash};
use crate::project::{Commit, DataSource, Project, Transform};
use crate::schema::{extract_parquet_schema, get_parquet_row_count};
use walkdir::WalkDir;

/// Compute the canonical hash for a commit from its fields.
///
/// This is the single source of truth for commit hash computation.
/// All code paths that need a commit hash (CLI commit, server push verification)
/// must use this function to avoid divergence.
pub fn compute_commit_hash(commit: &Commit) -> String {
    let commit_content = serde_json::json!({
        "parent_hashes": commit.parent_hashes,
        "data_sources": commit.data_sources,
        "transforms": commit.transforms,
        "endpoints": commit.endpoints,
        "author": commit.author,
        "message": commit.message,
        "timestamp": commit.timestamp.to_rfc3339(),
    });
    canon::hash_json(&commit_content)
}

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
        BTreeMap::new()
    };

    // Get parent hashes
    let parent_hashes = if let Some(head) = project.head_commit()? {
        vec![head]
    } else {
        vec![]
    };

    // Capture timestamp before hashing so it's included in the hash
    let timestamp = Utc::now();

    let mut commit = Commit {
        hash: String::new(),
        parent_hashes,
        author: author.to_string(),
        message: message.to_string(),
        timestamp,
        data_sources,
        transforms,
        endpoints,
    };
    commit.hash = compute_commit_hash(&commit);

    Ok(commit)
}

/// Collect data sources from the data/ directory.
pub fn collect_data_sources(project: &Project) -> Result<BTreeMap<String, DataSource>> {
    let mut sources = BTreeMap::new();
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
            let schema_value = serde_json::to_value(&schema)?;
            let schema_hash = canon::hash_json(&schema_value);

            let metadata = fs::metadata(&path)?;
            let row_count = get_parquet_row_count(&path).ok();

            let file_name = path
                .file_name()
                .ok_or_else(|| {
                    crate::error::Error::InvalidPath(format!(
                        "No file name in path: {}",
                        path.display()
                    ))
                })?
                .to_string_lossy();

            sources.insert(
                name.clone(),
                DataSource {
                    name,
                    hash,
                    schema_hash,
                    path: format!("data/{}", file_name),
                    row_count,
                    byte_size: Some(metadata.len()),
                },
            );
        }
    }

    Ok(sources)
}

/// Collect transforms from the transforms/ directory.
pub fn collect_transforms(project: &Project) -> Result<BTreeMap<String, Transform>> {
    let mut transforms = BTreeMap::new();
    let transforms_dir = project.transforms_dir();

    if !transforms_dir.exists() {
        return Ok(transforms);
    }

    // Detect platform once for all transforms (avoids spawning subprocess per file)
    let python_version = crate::platform::PlatformFingerprint::detect().python_version;

    for entry in WalkDir::new(&transforms_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !entry.file_type().is_file()
            || path.extension().map(|e| e == "py").unwrap_or(false) == false
        {
            continue;
        }

        // Parse Python file to find transform definitions
        let content = fs::read_to_string(path)?;
        let source_path = path
            .strip_prefix(&project.root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");

        // Look for @ozzy.transform decorated functions
        for transform in parse_python_transforms(&content, path, &source_path, &python_version)? {
            if transforms.contains_key(&transform.name) {
                return Err(crate::error::Error::TransformAlreadyExists(format!(
                    "'{}' defined in multiple files",
                    transform.name
                )));
            }
            transforms.insert(transform.name.clone(), transform);
        }
    }

    Ok(transforms)
}

/// Parse Python file for transform definitions.
fn parse_python_transforms(
    content: &str,
    path: &Path,
    source_path: &str,
    python_version: &Option<String>,
) -> Result<Vec<Transform>> {
    let mut transforms = Vec::new();
    let source_hash = hash_source_file(path)?;

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
            let mut saw_open_paren = false;
            while j < lines.len() {
                let l = lines[j];
                decorator_content.push_str(l);
                decorator_content.push('\n');

                let open_count = l.chars().filter(|&c| c == '(').count() as i32;
                let close_count = l.chars().filter(|&c| c == ')').count() as i32;
                if open_count > 0 {
                    saw_open_paren = true;
                }
                paren_depth += open_count;
                paren_depth = (paren_depth - close_count).max(0);

                // Single-line decorators can be balanced on the same line (j == i).
                if (!saw_open_paren && j == i) || (saw_open_paren && paren_depth == 0) {
                    break;
                }
                j += 1;
            }

            // Find the function definition
            j += 1;
            let decorator_line = i + 1; // 1-indexed line number for the decorator
            let mut found_def = false;
            while j < lines.len() {
                let l = lines[j].trim();
                if l.starts_with("def ") {
                    found_def = true;
                    // Extract function name
                    if let Some(name_end) = l.find('(') {
                        let function_name = l[4..name_end].trim().to_string();

                        // Parse decorator for metadata
                        let meta = parse_transform_decorator(&decorator_content);

                        // Look for lockfile (uv.lock in same directory)
                        let lockfile_hash = if let Some(parent) = path.parent() {
                            let lockfile_path = parent.join("uv.lock");
                            if lockfile_path.exists() {
                                blake3_hash_file(&lockfile_path)?
                            } else {
                                blake3_hash(b"")
                            }
                        } else {
                            // No parent directory - use empty hash
                            blake3_hash(b"")
                        };

                        // Build input_schema with inputs list and any requires
                        let input_schema = if meta.inputs.len() > 1 || meta.input_schema.is_some() {
                            let mut schema = serde_json::Map::new();
                            if meta.inputs.len() > 1
                                || meta.inputs.first().map(|s| s != "main").unwrap_or(false)
                            {
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

                        // Use cached python version for runtime field
                        let runtime = match python_version {
                            Some(ver) => format!("python-{}", ver),
                            None => "python".to_string(),
                        };

                        // Compute the full composite transform hash:
                        // blake3(source_hash + function_name + lockfile_hash + runtime + params_schema_hash)
                        let params_schema_hash = canon::hash_json(&meta.params_schema);
                        let full_hash = transform_hash(
                            &source_hash,
                            &function_name,
                            &lockfile_hash,
                            &runtime,
                            &params_schema_hash,
                        );

                        transforms.push(Transform {
                            name: function_name.clone(),
                            hash: full_hash,
                            runtime,
                            source_path: source_path.to_string(),
                            function_name,
                            lockfile_hash,
                            params_schema: meta.params_schema,
                            source_hash: source_hash.clone(),
                            reproducible: meta.reproducible,
                            input_schema,
                            output_schema: meta.output_schema,
                        });
                    }
                    break;
                }
                j += 1;
            }
            if !found_def {
                eprintln!(
                    "Warning: @ozzy.transform decorator at {}:{} is not followed by a function definition",
                    path.display(),
                    decorator_line
                );
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
        } else if let Some(inputs_dict) = extract_python_dict_raw(rest) {
            let parsed_inputs: Vec<String> = parse_simple_dict(&inputs_dict)
                .into_iter()
                .map(|(k, _)| k)
                .collect();
            if !parsed_inputs.is_empty() {
                meta.inputs = parsed_inputs;
            } else {
                eprintln!("Warning: could not parse 'inputs=' in @ozzy.transform decorator");
            }
        } else {
            eprintln!("Warning: could not parse 'inputs=' in @ozzy.transform decorator");
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
        } else {
            eprintln!("Warning: could not parse 'params=' in @ozzy.transform decorator");
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
                    } else {
                        eprintln!(
                            "Warning: could not parse 'input_schema=' in @ozzy.transform decorator"
                        );
                    }
                } else {
                    eprintln!(
                        "Warning: could not parse 'input_schema=' in @ozzy.transform decorator"
                    );
                }
            }
        } else {
            eprintln!("Warning: could not parse 'input_schema=' in @ozzy.transform decorator");
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
                    } else {
                        eprintln!(
                            "Warning: could not parse 'output_schema=' in @ozzy.transform decorator"
                        );
                    }
                } else {
                    eprintln!(
                        "Warning: could not parse 'output_schema=' in @ozzy.transform decorator"
                    );
                }
            }
        } else {
            eprintln!("Warning: could not parse 'output_schema=' in @ozzy.transform decorator");
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
        .map(|item| item.trim().trim_matches('"').trim_matches('\'').to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if items.is_empty() { None } else { Some(items) }
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

        // Compare full transform metadata, not just source hash.
        // This ensures path/runtime/function changes are tracked.
        if current_transforms != last_commit.transforms {
            return Ok(true);
        }

        Ok(false)
    } else {
        // No previous commit - there are changes if there's any data or transforms
        Ok(!current_data.is_empty() || !current_transforms.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::collect_transforms;
    use crate::Project;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn parses_single_line_transform_decorator() {
        let dir = tempdir().unwrap();
        let project = Project::init(dir.path(), "test-project", "testuser").unwrap();
        let transform_path = project.transforms_dir().join("single_line.py");
        fs::write(
            &transform_path,
            r#"
class ozzy:
    @staticmethod
    def transform(**kwargs):
        def decorator(fn):
            return fn
        return decorator

@ozzy.transform(params={"threshold": float})
def only_transform(inputs, params):
    return inputs["main"]
"#,
        )
        .unwrap();

        let transforms = collect_transforms(&project).unwrap();
        assert!(transforms.contains_key("only_transform"));
    }

    #[test]
    fn discovers_transforms_recursively() {
        let dir = tempdir().unwrap();
        let project = Project::init(dir.path(), "test-project", "testuser").unwrap();
        let nested_dir = project.transforms_dir().join("nested");
        fs::create_dir_all(&nested_dir).unwrap();
        let transform_path = nested_dir.join("deep.py");
        fs::write(
            &transform_path,
            r#"
class ozzy:
    @staticmethod
    def transform(**kwargs):
        def decorator(fn):
            return fn
        return decorator

@ozzy.transform()
def nested_transform(inputs, params):
    return inputs["main"]
"#,
        )
        .unwrap();

        let transforms = collect_transforms(&project).unwrap();
        let transform = transforms
            .get("nested_transform")
            .expect("missing transform");
        assert_eq!(transform.source_path, "transforms/nested/deep.py");
    }

    #[test]
    fn parses_inputs_dict_keys() {
        let dir = tempdir().unwrap();
        let project = Project::init(dir.path(), "test-project", "testuser").unwrap();
        let transform_path = project.transforms_dir().join("multi.py");
        fs::write(
            &transform_path,
            r#"
class ozzy:
    @staticmethod
    def transform(**kwargs):
        def decorator(fn):
            return fn
        return decorator

@ozzy.transform(inputs={"left": "raw", "right": "meta"})
def join_transform(inputs, params):
    return inputs["left"]
"#,
        )
        .unwrap();

        let transforms = collect_transforms(&project).unwrap();
        let transform = transforms.get("join_transform").expect("missing transform");
        let input_schema = transform
            .input_schema
            .as_ref()
            .expect("missing input schema");
        let inputs = input_schema
            .get("inputs")
            .and_then(|v| v.as_array())
            .expect("missing inputs array");
        let parsed: Vec<&str> = inputs.iter().filter_map(|v| v.as_str()).collect();
        assert_eq!(parsed, vec!["left", "right"]);
    }

    #[test]
    fn source_hash_is_populated_and_distinct_from_composite_hash() {
        let dir = tempdir().unwrap();
        let project = Project::init(dir.path(), "test-project", "testuser").unwrap();
        let transform_path = project.transforms_dir().join("basic.py");
        fs::write(
            &transform_path,
            r#"
class ozzy:
    @staticmethod
    def transform(**kwargs):
        def decorator(fn):
            return fn
        return decorator

@ozzy.transform()
def basic(inputs, params):
    return inputs["main"]
"#,
        )
        .unwrap();

        let transforms = collect_transforms(&project).unwrap();
        let t = transforms.get("basic").expect("missing transform");

        // source_hash should be non-empty
        assert!(!t.source_hash.is_empty(), "source_hash should be populated");
        // source_hash should differ from composite hash (composite includes function_name, runtime, etc.)
        assert_ne!(
            t.source_hash, t.hash,
            "source_hash should differ from composite hash"
        );
    }

    #[test]
    fn malformed_decorator_without_def_is_skipped() {
        let dir = tempdir().unwrap();
        let project = Project::init(dir.path(), "test-project", "testuser").unwrap();
        let transform_path = project.transforms_dir().join("malformed.py");
        fs::write(
            &transform_path,
            r#"
class ozzy:
    @staticmethod
    def transform(**kwargs):
        def decorator(fn):
            return fn
        return decorator

@ozzy.transform()
# missing def here - this is malformed
"#,
        )
        .unwrap();

        let transforms = collect_transforms(&project).unwrap();
        // Should not crash, and should not find any transforms
        assert!(
            transforms.is_empty(),
            "malformed decorator should produce no transforms"
        );
    }
}
