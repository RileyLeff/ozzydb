use anyhow::Result;
use ozzy_core::{Project, schema, validate_safe_name};
use std::fs;
use std::path::Path;

pub async fn add(file: &str, name: &str) -> Result<()> {
    let project = Project::find_current()?;
    let source_path = Path::new(file);

    // Validate name to prevent path traversal
    validate_safe_name(name)?;

    if !source_path.exists() {
        anyhow::bail!("File not found: {}", file);
    }

    if source_path
        .extension()
        .map(|e| e != "parquet")
        .unwrap_or(true)
    {
        anyhow::bail!("Only parquet files are supported. Got: {}", file);
    }

    // Copy to data/ directory
    let dest_path = project.data_dir().join(format!("{}.parquet", name));

    if dest_path.exists() {
        anyhow::bail!("Data source '{}' already exists", name);
    }

    fs::copy(source_path, &dest_path)?;

    // Extract and display schema
    let schema_info = schema::extract_parquet_schema(&dest_path)?;
    let row_count = schema::get_parquet_row_count(&dest_path)?;

    println!("Added data source: {}", name);
    println!("  Path: data/{}.parquet", name);
    println!("  Rows: {}", row_count);
    println!("  Columns: {}", schema_info.fields.len());
    println!();
    println!("{}", schema::format_schema(&schema_info));

    Ok(())
}

pub async fn list() -> Result<()> {
    let project = Project::find_current()?;
    let data_dir = project.data_dir();

    if !data_dir.exists() {
        println!("No data sources found.");
        return Ok(());
    }

    let mut found = false;
    for entry in fs::read_dir(&data_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().map(|e| e == "parquet").unwrap_or(false) {
            found = true;
            let name = path.file_stem().unwrap_or_default().to_string_lossy();
            let metadata = fs::metadata(&path)?;
            let size = format_size(metadata.len());

            let row_count = schema::get_parquet_row_count(&path).unwrap_or(0);

            println!("  {} ({} rows, {})", name, row_count, size);
        }
    }

    if !found {
        println!("No data sources found.");
    }

    Ok(())
}

pub async fn remove(name: &str) -> Result<()> {
    let project = Project::find_current()?;

    // Validate name to prevent path traversal
    validate_safe_name(name)?;

    let path = project.data_dir().join(format!("{}.parquet", name));

    if !path.exists() {
        anyhow::bail!("Data source '{}' not found", name);
    }

    fs::remove_file(&path)?;
    println!("Removed data source: {}", name);

    Ok(())
}

pub async fn schema(name: &str) -> Result<()> {
    let project = Project::find_current()?;

    // Validate name to prevent path traversal
    validate_safe_name(name)?;

    let path = project.data_dir().join(format!("{}.parquet", name));

    if !path.exists() {
        anyhow::bail!("Data source '{}' not found", name);
    }

    let schema_info = schema::extract_parquet_schema(&path)?;
    let row_count = schema::get_parquet_row_count(&path)?;

    println!("Data source: {}", name);
    println!("Rows: {}", row_count);
    println!();
    println!("{}", schema::format_schema(&schema_info));

    Ok(())
}

use ozzy_core::cache::format_size;
