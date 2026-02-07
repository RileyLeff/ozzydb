use anyhow::Result;
use ozzy_core::{Project, commit, validate_safe_name};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone)]
struct TransformBlock {
    name: String,
    decorator_start: usize,
    decorator_end: usize,
    def_line: usize,
}

pub async fn add(file: &str, name: Option<&str>) -> Result<()> {
    let project = Project::find_current()?;

    // Parse file:function format or just file
    let (source_path, function_name) = if file.contains(':') {
        let parts: Vec<&str> = file.splitn(2, ':').collect();
        (Path::new(parts[0]), Some(parts[1].to_string()))
    } else {
        (Path::new(file), None)
    };

    if !source_path.exists() {
        anyhow::bail!("File not found: {}", source_path.display());
    }

    if source_path.extension().map(|e| e != "py").unwrap_or(true) {
        anyhow::bail!("Only Python transforms (.py) are supported for now");
    }

    // Read the source to find decorated functions
    let content = fs::read_to_string(source_path)?;

    // Find @ozzy.transform decorated functions
    let functions = find_transform_functions(&content);

    if functions.is_empty() {
        anyhow::bail!(
            "No @ozzy.transform decorated functions found in {}",
            source_path.display()
        );
    }

    // If function name specified, verify it exists
    if let Some(ref fn_name) = function_name {
        if !functions.iter().any(|f| f == fn_name) {
            anyhow::bail!(
                "Function '{}' not found. Available transforms: {}",
                fn_name,
                functions.join(", ")
            );
        }
    }

    let selected_function = if let Some(ref fn_name) = function_name {
        Some(fn_name.clone())
    } else if functions.len() == 1 {
        Some(functions[0].to_string())
    } else {
        None
    };

    let (dest_file_name, content_to_write, displayed_functions): (String, String, Vec<String>) =
        if let Some(alias) = name {
            validate_safe_name(alias)?;

            if functions.len() > 1 && function_name.is_none() {
                anyhow::bail!(
                    "--name is only supported for files that contain a single @ozzy.transform function"
                );
            }

            let source_function = selected_function.as_ref().ok_or_else(|| {
                anyhow::anyhow!(
                    "Could not determine source transform. Use <file.py:function> with --name."
                )
            })?;
            let renamed =
                rewrite_selected_transform_source(&content, source_function, Some(alias))?;
            (format!("{}.py", alias), renamed, vec![alias.to_string()])
        } else if let Some(source_function) = selected_function.as_ref() {
            let rewritten = if functions.len() > 1 {
                rewrite_selected_transform_source(&content, source_function, None)?
            } else {
                content.clone()
            };
            (
                source_path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
                rewritten,
                functions.clone(),
            )
        } else {
            (
                source_path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
                content.clone(),
                functions.clone(),
            )
        };

    // Copy to transforms/ directory
    let dest_path = project.transforms_dir().join(&dest_file_name);

    if dest_path.exists() {
        // Check if content is different
        let existing = fs::read_to_string(&dest_path)?;
        if existing == content_to_write {
            println!("Transform file already exists and is identical.");
        } else {
            fs::write(&dest_path, &content_to_write)?;
            println!("Updated transform file: transforms/{}", dest_file_name);
        }
    } else {
        fs::write(&dest_path, &content_to_write)?;
    }

    // Show what transforms were found
    println!("Found transforms:");
    for func in &displayed_functions {
        let selected = if let Some(alias) = name {
            func == alias
        } else {
            function_name.as_ref().map(|n| n == func).unwrap_or(true)
        };
        if selected {
            println!("  {} ✓", func);
        } else {
            println!("  {} (not selected)", func);
        }
    }

    Ok(())
}

pub async fn list() -> Result<()> {
    let project = Project::find_current()?;
    let transforms = commit::collect_transforms(&project)?;

    if transforms.is_empty() {
        println!("No transforms found.");
        println!();
        println!("Add transforms with: ozzy transform add <file.py>");
        return Ok(());
    }

    println!("Transforms:");
    for (name, transform) in &transforms {
        let reproducible = if transform.reproducible {
            ""
        } else {
            " (non-deterministic)"
        };
        println!("  {} [{}]{}", name, transform.runtime, reproducible);
        println!("    Source: {}", transform.source_path);
    }

    Ok(())
}

pub async fn remove(name: &str) -> Result<()> {
    let project = Project::find_current()?;

    // Validate name to prevent path traversal
    validate_safe_name(name)?;

    let transforms = commit::collect_transforms(&project)?;

    if !transforms.contains_key(name) {
        anyhow::bail!("Transform '{}' not found", name);
    }

    let transform = &transforms[name];
    let path = project.root.join(&transform.source_path);

    // Read the file to check if there are other transforms in it
    let content = fs::read_to_string(&path)?;
    let functions = find_transform_functions(&content);

    if functions.len() > 1 {
        anyhow::bail!(
            "Cannot remove '{}' - file contains multiple transforms: {}. Remove the file manually.",
            name,
            functions.join(", ")
        );
    }

    fs::remove_file(&path)?;
    println!("Removed transform: {}", name);

    Ok(())
}

pub async fn test(name: &str, _sample: usize) -> Result<()> {
    let project = Project::find_current()?;

    // Validate name to prevent path traversal
    validate_safe_name(name)?;

    let transforms = commit::collect_transforms(&project)?;
    let transform = transforms
        .get(name)
        .ok_or_else(|| anyhow::anyhow!("Transform '{}' not found", name))?;

    let data_sources = commit::collect_data_sources(&project)?;

    println!("Testing transform: {}", name);
    println!("  Source: {}", transform.source_path);
    println!("  Function: {}", transform.function_name);
    if !transform.reproducible {
        println!("  Warning: marked as non-reproducible (reproducible=False)");
    }
    println!();

    // Find a suitable data source to test with
    let test_input = match data_sources.values().next() {
        Some(ds) => ds,
        None => {
            println!("No data sources found. Add data with 'ozzy data add' to test transforms.");
            return Ok(());
        }
    };
    let input_path = project.root.join(&test_input.path);

    if !input_path.exists() {
        anyhow::bail!("Data source file not found: {}", input_path.display());
    }

    println!(
        "Input: {} ({} rows)",
        test_input.name,
        test_input.row_count.unwrap_or(0)
    );
    println!();

    // Execute the transform
    let temp_dir = tempfile::tempdir()?;
    let temp_output = temp_dir.path().join("test_output.parquet");

    let mut input_paths = std::collections::HashMap::new();
    input_paths.insert("main".to_string(), input_path);

    let params = serde_json::json!({});
    let start = std::time::Instant::now();

    match ozzy_core::runtime::execute_transform_multi(
        &project.root.join(&transform.source_path),
        &transform.function_name,
        &input_paths,
        &temp_output,
        &params,
    ) {
        Ok(()) => {
            let elapsed = start.elapsed();
            println!("Execution: OK ({:.2}s)", elapsed.as_secs_f64());

            if temp_output.exists() {
                let metadata = fs::metadata(&temp_output)?;
                let row_count = ozzy_core::schema::get_parquet_row_count(&temp_output).unwrap_or(0);
                println!(
                    "Output: {} rows, {}",
                    row_count,
                    ozzy_core::cache::format_size(metadata.len())
                );

                if let Ok(schema) = ozzy_core::schema::extract_parquet_schema(&temp_output) {
                    println!();
                    println!("Output schema:");
                    for field in &schema.fields {
                        let nullable = if field.nullable { " (nullable)" } else { "" };
                        println!("  {}: {}{}", field.name, field.dtype, nullable);
                    }
                }
            }
        }
        Err(e) => {
            let elapsed = start.elapsed();
            println!("Execution: FAILED ({:.2}s)", elapsed.as_secs_f64());
            println!();
            println!("Error: {}", e);
        }
    }

    Ok(())
}

fn find_transform_functions(content: &str) -> Vec<String> {
    find_transform_blocks(content)
        .into_iter()
        .map(|b| b.name)
        .collect()
}

fn find_transform_blocks(content: &str) -> Vec<TransformBlock> {
    let mut blocks = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();
        if !line.starts_with("@ozzy.transform") {
            i += 1;
            continue;
        }

        // Consume decorator block (supports single-line and multiline decorators).
        let mut j = i;
        let mut paren_depth = 0;
        let mut saw_open_paren = false;
        while j < lines.len() {
            let l = lines[j];
            let open_count = l.chars().filter(|&c| c == '(').count() as i32;
            let close_count = l.chars().filter(|&c| c == ')').count() as i32;
            if open_count > 0 {
                saw_open_paren = true;
            }
            paren_depth += open_count;
            paren_depth -= close_count;

            if (!saw_open_paren && j == i) || (saw_open_paren && paren_depth == 0) {
                break;
            }
            j += 1;
        }

        let mut next_i = i + 1;
        // Look for the function definition after the decorator block.
        for k in (j + 1)..lines.len() {
            let l = lines[k].trim();
            if l.starts_with("def ") {
                if let Some(paren_pos) = l.find('(') {
                    let name = l[4..paren_pos].trim();
                    blocks.push(TransformBlock {
                        name: name.to_string(),
                        decorator_start: i,
                        decorator_end: j,
                        def_line: k,
                    });
                }
                next_i = k + 1;
                break;
            }
        }
        i = next_i;
    }

    blocks
}

fn rewrite_selected_transform_source(
    content: &str,
    selected_name: &str,
    rename_to: Option<&str>,
) -> Result<String> {
    let blocks = find_transform_blocks(content);
    if blocks.is_empty() {
        anyhow::bail!("No @ozzy.transform decorated functions found");
    }

    let mut selected = false;
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    for block in &blocks {
        if block.name == selected_name {
            selected = true;
            continue;
        }

        // Disable non-selected transform decorators so only the chosen transform
        // is registered from this file.
        for idx in block.decorator_start..=block.decorator_end {
            let original = &lines[idx];
            if original.trim_start().starts_with('#') {
                continue;
            }
            lines[idx] = format!("# {}", original);
        }
    }

    if !selected {
        anyhow::bail!("Transform '{}' not found", selected_name);
    }

    if let Some(new_name) = rename_to {
        let block = blocks
            .iter()
            .find(|b| b.name == selected_name)
            .ok_or_else(|| anyhow::anyhow!("Transform '{}' not found", selected_name))?;
        let def_line = &lines[block.def_line];
        let trimmed = def_line.trim_start();
        if !trimmed.starts_with("def ") {
            anyhow::bail!(
                "Could not rename function '{}' in transform source",
                selected_name
            );
        }
        let Some(paren_pos) = trimmed.find('(') else {
            anyhow::bail!(
                "Could not rename function '{}' in transform source",
                selected_name
            );
        };
        let indent_len = def_line.len() - trimmed.len();
        let indent = &def_line[..indent_len];
        let suffix = &trimmed[paren_pos..];
        lines[block.def_line] = format!("{indent}def {new_name}{suffix}");
    }

    let mut rewritten = lines.join("\n");
    if content.ends_with('\n') {
        rewritten.push('\n');
    }
    Ok(rewritten)
}
