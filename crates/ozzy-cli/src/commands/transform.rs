use anyhow::Result;
use ozzy_core::{commit, validate_safe_name, Project};
use std::fs;
use std::path::Path;

pub async fn add(file: &str, _name: Option<&str>) -> Result<()> {
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
        if !functions.contains(&fn_name.as_str()) {
            anyhow::bail!(
                "Function '{}' not found. Available transforms: {}",
                fn_name,
                functions.join(", ")
            );
        }
    }

    // Copy to transforms/ directory
    let dest_name = source_path.file_name().unwrap();
    let dest_path = project.transforms_dir().join(dest_name);

    if dest_path.exists() {
        // Check if content is different
        let existing = fs::read_to_string(&dest_path)?;
        if existing == content {
            println!("Transform file already exists and is identical.");
        } else {
            fs::copy(source_path, &dest_path)?;
            println!("Updated transform file: transforms/{}", dest_name.to_string_lossy());
        }
    } else {
        fs::copy(source_path, &dest_path)?;
    }

    // Show what transforms were found
    println!("Found transforms:");
    for func in &functions {
        let selected = function_name.as_ref().map(|n| n == *func).unwrap_or(true);
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
        let reproducible = if transform.reproducible { "" } else { " (non-deterministic)" };
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

    println!("Input: {} ({} rows)", test_input.name, test_input.row_count.unwrap_or(0));
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
                println!("Output: {} rows, {}", row_count, ozzy_core::cache::format_size(metadata.len()));

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

fn find_transform_functions(content: &str) -> Vec<&str> {
    let mut functions = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

    for (i, line) in lines.iter().enumerate() {
        if line.trim().starts_with("@ozzy.transform") {
            // Look for the function definition
            for j in (i + 1)..lines.len() {
                let l = lines[j].trim();
                if l.starts_with("def ") {
                    if let Some(paren_pos) = l.find('(') {
                        let name = l[4..paren_pos].trim();
                        functions.push(name);
                    }
                    break;
                }
            }
        }
    }

    functions
}
