//! Integration tests for OzzyDB CLI.
//!
//! These tests verify the full workflow:
//! 1. Initialize a project
//! 2. Add data sources
//! 3. Add transforms
//! 4. Create endpoints with schema validation
//! 5. Run pipelines
//! 6. Verify caching works

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

fn ozzy() -> Command {
    Command::cargo_bin("ozzy").unwrap()
}

/// Create a test parquet file with sample data.
fn create_test_parquet(path: &Path) {
    // Use Python/polars to create a test parquet file
    let script = format!(
        r#"
import polars as pl

df = pl.DataFrame({{
    "id": [1, 2, 3, 4, 5],
    "name": ["alice", "bob", "charlie", "diana", "eve"],
    "value": [10.5, 20.3, 15.7, 8.2, 12.9],
    "category": ["A", "B", "A", "C", "B"],
}})

df.write_parquet("{}")
"#,
        path.display()
    );

    std::process::Command::new("uv")
        .args([
            "run", "--with", "polars", "--with", "pyarrow", "python", "-c", &script,
        ])
        .output()
        .expect("Failed to create test parquet file");
}

/// Create a test transform file.
fn create_test_transform(path: &Path) {
    let content = r#"
import polars as pl

class ozzy:
    @staticmethod
    def transform(**kwargs):
        def decorator(func):
            return func
        return decorator

@ozzy.transform(
    params={"threshold": float},
)
def filter_by_value(inputs, params):
    """Filter rows where value exceeds threshold."""
    df = inputs["main"]
    threshold = getattr(params, "threshold", 10.0)
    return df.filter(pl.col("value") > threshold)

@ozzy.transform(
    params={"prefix": str},
)
def add_prefix(inputs, params):
    """Add a prefix to the name column."""
    df = inputs["main"]
    prefix = getattr(params, "prefix", "user_")
    return df.with_columns(
        (pl.lit(prefix) + pl.col("name")).alias("prefixed_name")
    )
"#;

    fs::write(path, content).expect("Failed to write transform file");
}

#[test]
fn test_init_project() {
    let dir = tempdir().unwrap();

    ozzy()
        .current_dir(dir.path())
        .args(["init", "--name", "test-project", "--owner", "testuser"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Initialized OzzyDB project"));

    // Verify files were created
    assert!(dir.path().join("ozzy.toml").exists());
    assert!(dir.path().join(".ozzy/commits").exists());
    assert!(dir.path().join("data").exists());
    assert!(dir.path().join("transforms").exists());
    let gitignore = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    assert!(gitignore.contains(".ozzy/"));
    assert!(gitignore.contains("data/*.parquet"));
}

#[test]
fn test_init_already_exists() {
    let dir = tempdir().unwrap();

    // First init should succeed
    ozzy()
        .current_dir(dir.path())
        .args(["init", "--name", "test-project", "--owner", "testuser"])
        .assert()
        .success();

    // Second init should indicate already initialized
    ozzy()
        .current_dir(dir.path())
        .args(["init", "--name", "test-project", "--owner", "testuser"])
        .assert()
        .success()
        .stdout(predicate::str::contains("already initialized"));
}

#[test]
fn test_data_operations() {
    let dir = tempdir().unwrap();

    // Initialize project
    ozzy()
        .current_dir(dir.path())
        .args(["init", "--name", "test-project", "--owner", "testuser"])
        .assert()
        .success();

    // Create test parquet file
    let parquet_path = dir.path().join("test_data.parquet");
    create_test_parquet(&parquet_path);

    // Add data source
    ozzy()
        .current_dir(dir.path())
        .args([
            "data",
            "add",
            parquet_path.to_str().unwrap(),
            "--name",
            "raw",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added data source: raw"));

    // List data sources
    ozzy()
        .current_dir(dir.path())
        .args(["data", "ls"])
        .assert()
        .success()
        .stdout(predicate::str::contains("raw"));

    // Show schema
    ozzy()
        .current_dir(dir.path())
        .args(["data", "schema", "raw"])
        .assert()
        .success()
        .stdout(predicate::str::contains("id"))
        .stdout(predicate::str::contains("name"))
        .stdout(predicate::str::contains("value"));
}

#[test]
fn test_transform_operations() {
    let dir = tempdir().unwrap();

    // Initialize project
    ozzy()
        .current_dir(dir.path())
        .args(["init", "--name", "test-project", "--owner", "testuser"])
        .assert()
        .success();

    // Create test transform
    let transform_path = dir.path().join("transforms/qc.py");
    fs::create_dir_all(transform_path.parent().unwrap()).unwrap();
    create_test_transform(&transform_path);

    // Add transform
    ozzy()
        .current_dir(dir.path())
        .args(["transform", "add", "transforms/qc.py"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Found transforms"));

    // List transforms
    ozzy()
        .current_dir(dir.path())
        .args(["transform", "ls"])
        .assert()
        .success()
        .stdout(predicate::str::contains("filter_by_value"))
        .stdout(predicate::str::contains("add_prefix"));
}

#[test]
fn test_transform_add_selector_from_multi_transform_file() {
    let dir = tempdir().unwrap();

    ozzy()
        .current_dir(dir.path())
        .args(["init", "--name", "selector-project", "--owner", "testuser"])
        .assert()
        .success();

    let transform_path = dir.path().join("transforms/qc.py");
    fs::create_dir_all(transform_path.parent().unwrap()).unwrap();
    create_test_transform(&transform_path);

    ozzy()
        .current_dir(dir.path())
        .args(["transform", "add", "transforms/qc.py:add_prefix"])
        .assert()
        .success()
        .stdout(predicate::str::contains("add_prefix"))
        .stdout(predicate::str::contains("filter_by_value (not selected)"));

    ozzy()
        .current_dir(dir.path())
        .args(["transform", "ls"])
        .assert()
        .success()
        .stdout(predicate::str::contains("add_prefix"))
        .stdout(predicate::str::contains("filter_by_value").not());
}

#[test]
fn test_full_pipeline() {
    let dir = tempdir().unwrap();

    // Initialize project
    ozzy()
        .current_dir(dir.path())
        .args(["init", "--name", "test-project", "--owner", "testuser"])
        .assert()
        .success();

    // Create and add test data
    let parquet_path = dir.path().join("test_data.parquet");
    create_test_parquet(&parquet_path);

    ozzy()
        .current_dir(dir.path())
        .args([
            "data",
            "add",
            parquet_path.to_str().unwrap(),
            "--name",
            "raw",
        ])
        .assert()
        .success();

    // Create and add transform
    let transform_path = dir.path().join("transforms/qc.py");
    fs::create_dir_all(transform_path.parent().unwrap()).unwrap();
    create_test_transform(&transform_path);

    ozzy()
        .current_dir(dir.path())
        .args(["transform", "add", "transforms/qc.py"])
        .assert()
        .success();

    // Create endpoint
    ozzy()
        .current_dir(dir.path())
        .args([
            "endpoint",
            "create",
            "filtered",
            "--input",
            "raw",
            "--transforms",
            "filter_by_value",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Created endpoint: filtered"));

    // List endpoints
    ozzy()
        .current_dir(dir.path())
        .args(["endpoint", "ls"])
        .assert()
        .success()
        .stdout(predicate::str::contains("filtered"));

    // Show DAG
    ozzy()
        .current_dir(dir.path())
        .args(["dag"])
        .assert()
        .success()
        .stdout(predicate::str::contains("raw"))
        .stdout(predicate::str::contains("filter_by_value"));

    // Run pipeline
    let output_path = dir.path().join("output.parquet");
    ozzy()
        .current_dir(dir.path())
        .args(["run", "filtered", "--output", output_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("Execution plan"));

    // Verify output was created
    assert!(output_path.exists(), "Output parquet file should exist");
}

#[test]
fn test_commit_and_log() {
    let dir = tempdir().unwrap();

    // Initialize project
    ozzy()
        .current_dir(dir.path())
        .args(["init", "--name", "test-project", "--owner", "testuser"])
        .assert()
        .success();

    // Create and add test data
    let parquet_path = dir.path().join("test_data.parquet");
    create_test_parquet(&parquet_path);

    ozzy()
        .current_dir(dir.path())
        .args([
            "data",
            "add",
            parquet_path.to_str().unwrap(),
            "--name",
            "raw",
        ])
        .assert()
        .success();

    // Create commit
    ozzy()
        .current_dir(dir.path())
        .args(["commit", "-m", "Add raw data"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Committed:"));

    // Check log
    ozzy()
        .current_dir(dir.path())
        .args(["log"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Add raw data"));

    // Status should show clean
    ozzy()
        .current_dir(dir.path())
        .args(["status"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("clean").or(predicate::str::contains("Nothing to commit")),
        );
}

#[test]
fn test_cache_operations() {
    let dir = tempdir().unwrap();

    // Initialize project
    ozzy()
        .current_dir(dir.path())
        .args(["init", "--name", "test-project", "--owner", "testuser"])
        .assert()
        .success();

    // Cache size (should be empty or zero initially)
    ozzy()
        .current_dir(dir.path())
        .args(["cache", "size"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Cache statistics"));

    // Cache list
    ozzy()
        .current_dir(dir.path())
        .args(["cache", "ls"])
        .assert()
        .success();
}

#[test]
fn test_endpoint_missing_data_source() {
    let dir = tempdir().unwrap();

    // Initialize project
    ozzy()
        .current_dir(dir.path())
        .args(["init", "--name", "test-project", "--owner", "testuser"])
        .assert()
        .success();

    // Try to create endpoint with non-existent data source
    ozzy()
        .current_dir(dir.path())
        .args([
            "endpoint",
            "create",
            "test",
            "--input",
            "nonexistent",
            "--transforms",
            "qc",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn test_endpoint_missing_transform() {
    let dir = tempdir().unwrap();

    // Initialize project
    ozzy()
        .current_dir(dir.path())
        .args(["init", "--name", "test-project", "--owner", "testuser"])
        .assert()
        .success();

    // Create and add test data
    let parquet_path = dir.path().join("test_data.parquet");
    create_test_parquet(&parquet_path);

    ozzy()
        .current_dir(dir.path())
        .args([
            "data",
            "add",
            parquet_path.to_str().unwrap(),
            "--name",
            "raw",
        ])
        .assert()
        .success();

    // Try to create endpoint with non-existent transform
    ozzy()
        .current_dir(dir.path())
        .args([
            "endpoint",
            "create",
            "test",
            "--input",
            "raw",
            "--transforms",
            "nonexistent",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn test_caching_works() {
    let dir = tempdir().unwrap();
    let cache_dir = tempdir().unwrap();

    // Initialize project
    ozzy()
        .current_dir(dir.path())
        .args(["init", "--name", "test-project", "--owner", "testuser"])
        .assert()
        .success();

    // Create and add test data
    let parquet_path = dir.path().join("test_data.parquet");
    create_test_parquet(&parquet_path);

    ozzy()
        .current_dir(dir.path())
        .args([
            "data",
            "add",
            parquet_path.to_str().unwrap(),
            "--name",
            "raw",
        ])
        .assert()
        .success();

    // Create and add transform
    let transform_path = dir.path().join("transforms/qc.py");
    fs::create_dir_all(transform_path.parent().unwrap()).unwrap();
    create_test_transform(&transform_path);

    ozzy()
        .current_dir(dir.path())
        .args(["transform", "add", "transforms/qc.py"])
        .assert()
        .success();

    // Create endpoint
    ozzy()
        .current_dir(dir.path())
        .args([
            "endpoint",
            "create",
            "filtered",
            "--input",
            "raw",
            "--transforms",
            "filter_by_value",
        ])
        .assert()
        .success();

    // First run - should be a cache MISS (isolated cache dir)
    ozzy()
        .current_dir(dir.path())
        .env("OZZY_CACHE_DIR", cache_dir.path())
        .args(["run", "filtered"])
        .assert()
        .success()
        .stdout(predicate::str::contains("MISS"));

    // Second run - should be a cache HIT
    ozzy()
        .current_dir(dir.path())
        .env("OZZY_CACHE_DIR", cache_dir.path())
        .args(["run", "filtered"])
        .assert()
        .success()
        .stdout(predicate::str::contains("HIT"));

    // Run with --force should skip cache
    ozzy()
        .current_dir(dir.path())
        .env("OZZY_CACHE_DIR", cache_dir.path())
        .args(["run", "filtered", "--force"])
        .assert()
        .success()
        .stdout(predicate::str::contains("SKIP"));
}

/// Create a second test parquet file with metadata for multi-input tests.
fn create_metadata_parquet(path: &Path) {
    let script = format!(
        r#"
import polars as pl

df = pl.DataFrame({{
    "id": [1, 2, 3, 4, 5],
    "description": ["desc_a", "desc_b", "desc_c", "desc_d", "desc_e"],
    "weight": [1.0, 2.0, 1.5, 0.5, 3.0],
}})

df.write_parquet("{}")
"#,
        path.display()
    );

    std::process::Command::new("uv")
        .args([
            "run", "--with", "polars", "--with", "pyarrow", "python", "-c", &script,
        ])
        .output()
        .expect("Failed to create metadata parquet file");
}

/// Create a multi-input transform.
fn create_multi_input_transform(path: &Path) {
    let content = r#"
import polars as pl

class ozzy:
    @staticmethod
    def transform(**kwargs):
        def decorator(func):
            return func
        return decorator

@ozzy.transform(
    inputs=["main", "meta"],
    params={"multiplier": float},
    input_schema={"requires": ["id", "value"]},
    output_schema={"adds": ["weighted_value"]},
)
def merge_with_weights(inputs, params):
    """Merge main data with metadata weights."""
    main = inputs["main"]
    meta = inputs["meta"]
    multiplier = getattr(params, "multiplier", 1.0)

    # Join on id and compute weighted value
    merged = main.join(meta, on="id")
    return merged.with_columns(
        (pl.col("value") * pl.col("weight") * multiplier).alias("weighted_value")
    )
"#;
    fs::write(path, content).expect("Failed to write multi-input transform");
}

#[test]
fn test_multi_input_endpoint() {
    let dir = tempdir().unwrap();

    // Initialize project
    ozzy()
        .current_dir(dir.path())
        .args(["init", "--name", "test-project", "--owner", "testuser"])
        .assert()
        .success();

    // Create and add main data
    let main_path = dir.path().join("main_data.parquet");
    create_test_parquet(&main_path);
    ozzy()
        .current_dir(dir.path())
        .args([
            "data",
            "add",
            main_path.to_str().unwrap(),
            "--name",
            "main_data",
        ])
        .assert()
        .success();

    // Create and add metadata
    let meta_path = dir.path().join("meta_data.parquet");
    create_metadata_parquet(&meta_path);
    ozzy()
        .current_dir(dir.path())
        .args([
            "data",
            "add",
            meta_path.to_str().unwrap(),
            "--name",
            "metadata",
        ])
        .assert()
        .success();

    // Create multi-input transform
    let transform_path = dir.path().join("transforms/merge.py");
    fs::create_dir_all(transform_path.parent().unwrap()).unwrap();
    create_multi_input_transform(&transform_path);
    ozzy()
        .current_dir(dir.path())
        .args(["transform", "add", "transforms/merge.py"])
        .assert()
        .success();

    // Create endpoint with multiple inputs
    ozzy()
        .current_dir(dir.path())
        .args([
            "endpoint",
            "create",
            "merged",
            "--input",
            "main:main_data",
            "--input",
            "meta:metadata",
            "--transforms",
            "merge_with_weights",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Created endpoint: merged"));

    // Show endpoint should show both inputs
    ozzy()
        .current_dir(dir.path())
        .args(["endpoint", "show", "merged"])
        .assert()
        .success()
        .stdout(predicate::str::contains("main"))
        .stdout(predicate::str::contains("meta"));

    // Run the multi-input pipeline
    let output_path = dir.path().join("merged_output.parquet");
    ozzy()
        .current_dir(dir.path())
        .args(["run", "merged", "--output", output_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("Inputs: 2"));

    // Verify output was created
    assert!(output_path.exists(), "Output parquet file should exist");
}

#[test]
fn test_cli_params() {
    let dir = tempdir().unwrap();
    let cache_dir = tempdir().unwrap();

    // Initialize project
    ozzy()
        .current_dir(dir.path())
        .args(["init", "--name", "test-project", "--owner", "testuser"])
        .assert()
        .success();

    // Create and add test data
    let parquet_path = dir.path().join("test_data.parquet");
    create_test_parquet(&parquet_path);
    ozzy()
        .current_dir(dir.path())
        .args([
            "data",
            "add",
            parquet_path.to_str().unwrap(),
            "--name",
            "raw",
        ])
        .assert()
        .success();

    // Create and add transform
    let transform_path = dir.path().join("transforms/qc.py");
    fs::create_dir_all(transform_path.parent().unwrap()).unwrap();
    create_test_transform(&transform_path);
    ozzy()
        .current_dir(dir.path())
        .args(["transform", "add", "transforms/qc.py"])
        .assert()
        .success();

    // Create endpoint
    ozzy()
        .current_dir(dir.path())
        .args([
            "endpoint",
            "create",
            "filtered",
            "--input",
            "raw",
            "--transforms",
            "filter_by_value",
        ])
        .assert()
        .success();

    // Run with custom threshold parameter
    ozzy()
        .current_dir(dir.path())
        .env("OZZY_CACHE_DIR", cache_dir.path())
        .args(["run", "filtered", "--param", "threshold=15.0"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Parameters"))
        .stdout(predicate::str::contains("threshold = 15.0"));

    // Different param should cause cache miss (different materialized hash)
    ozzy()
        .current_dir(dir.path())
        .env("OZZY_CACHE_DIR", cache_dir.path())
        .args(["run", "filtered", "--param", "threshold=5.0"])
        .assert()
        .success()
        .stdout(predicate::str::contains("MISS"));
}

#[test]
fn test_transform_with_schema_hints() {
    let dir = tempdir().unwrap();

    // Initialize project
    ozzy()
        .current_dir(dir.path())
        .args(["init", "--name", "test-project", "--owner", "testuser"])
        .assert()
        .success();

    // Create and add test data
    let parquet_path = dir.path().join("test_data.parquet");
    create_test_parquet(&parquet_path);
    ozzy()
        .current_dir(dir.path())
        .args([
            "data",
            "add",
            parquet_path.to_str().unwrap(),
            "--name",
            "raw",
        ])
        .assert()
        .success();

    // Create transform with schema hints
    let transform_path = dir.path().join("transforms/schema_transform.py");
    fs::create_dir_all(transform_path.parent().unwrap()).unwrap();
    let content = r#"
import polars as pl

class ozzy:
    @staticmethod
    def transform(**kwargs):
        def decorator(func):
            return func
        return decorator

@ozzy.transform(
    params={"factor": float},
    input_schema={"requires": ["id", "value"]},
    output_schema={"adds": ["scaled_value"]},
)
def scale_values(inputs, params):
    """Scale the value column by a factor."""
    df = inputs["main"]
    factor = getattr(params, "factor", 1.0)
    return df.with_columns(
        (pl.col("value") * factor).alias("scaled_value")
    )
"#;
    fs::write(&transform_path, content).expect("Failed to write transform");

    ozzy()
        .current_dir(dir.path())
        .args(["transform", "add", "transforms/schema_transform.py"])
        .assert()
        .success()
        .stdout(predicate::str::contains("scale_values"));

    // List transforms should show the new transform
    ozzy()
        .current_dir(dir.path())
        .args(["transform", "ls"])
        .assert()
        .success()
        .stdout(predicate::str::contains("scale_values"));

    // Create endpoint - schema validation should pass since required columns exist
    ozzy()
        .current_dir(dir.path())
        .args([
            "endpoint",
            "create",
            "scaled",
            "--input",
            "raw",
            "--transforms",
            "scale_values",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Created endpoint: scaled"));
}

fn create_sap_flux_parquet(path: &Path) {
    let script = format!(
        r#"
import polars as pl

df = pl.DataFrame({{
    "timestamp": ["2026-01-01T00:00:00Z", "2026-01-01T00:01:00Z", "2026-01-01T00:02:00Z", "2026-01-01T00:03:00Z"],
    "sensor_id": ["A", "A", "B", "B"],
    "raw_mv": [10.0, 11.5, 8.0, 12.2],
    "temp_c": [20.0, 20.5, 21.0, 22.0],
}})

df.write_parquet("{}")
"#,
        path.display()
    );

    std::process::Command::new("uv")
        .args([
            "run", "--with", "polars", "--with", "pyarrow", "python", "-c", &script,
        ])
        .output()
        .expect("Failed to create sap flux parquet file");
}

fn create_sap_flux_transforms(path: &Path) {
    let content = r#"
import polars as pl

class ozzy:
    @staticmethod
    def transform(**kwargs):
        def decorator(func):
            return func
        return decorator

@ozzy.transform(params={"min_raw_mv": float})
def qc(inputs, params):
    df = inputs["main"]
    min_raw_mv = getattr(params, "min_raw_mv", 9.0)
    return (
        df.filter(pl.col("raw_mv") >= min_raw_mv)
          .with_columns(pl.col("raw_mv").alias("signal_mv_qc"))
    )

@ozzy.transform(params={"gain": float, "offset": float})
def calibrate(inputs, params):
    df = inputs["main"]
    gain = getattr(params, "gain", 1.05)
    offset = getattr(params, "offset", 0.1)
    return df.with_columns(
        (pl.col("signal_mv_qc") * gain + offset).alias("signal_mv_cal")
    )

@ozzy.transform(params={"ambient_offset": float})
def corrected_signal(inputs, params):
    df = inputs["main"]
    ambient_offset = getattr(params, "ambient_offset", 0.2)
    return df.with_columns(
        (pl.col("signal_mv_cal") - ambient_offset).alias("corrected")
    )
"#;

    fs::write(path, content).expect("Failed to write sap flux transforms");
}

fn parquet_row_count(path: &Path) -> usize {
    let script = format!(
        r#"
import polars as pl
print(pl.read_parquet(r'''{}''').height)
"#,
        path.display()
    );

    let output = std::process::Command::new("uv")
        .args([
            "run", "--with", "polars", "--with", "pyarrow", "python", "-c", &script,
        ])
        .output()
        .expect("Failed to read parquet row count");
    assert!(output.status.success(), "Failed to read parquet row count");
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<usize>()
        .expect("Invalid row count output")
}

#[test]
fn test_remotes_persist_after_workspace_save() {
    let dir = tempdir().unwrap();

    ozzy()
        .current_dir(dir.path())
        .args(["init", "--name", "test-project", "--owner", "testuser"])
        .assert()
        .success();

    ozzy()
        .current_dir(dir.path())
        .args(["remote", "add", "origin", "https://registry.example.com"])
        .assert()
        .success();

    let parquet_path = dir.path().join("test_data.parquet");
    create_test_parquet(&parquet_path);
    ozzy()
        .current_dir(dir.path())
        .args([
            "data",
            "add",
            parquet_path.to_str().unwrap(),
            "--name",
            "raw",
        ])
        .assert()
        .success();

    ozzy()
        .current_dir(dir.path())
        .args(["remote", "ls"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "origin -> https://registry.example.com",
        ));
}

#[test]
fn test_dag_svg_output() {
    let dir = tempdir().unwrap();

    ozzy()
        .current_dir(dir.path())
        .args(["init", "--name", "test-project", "--owner", "testuser"])
        .assert()
        .success();

    let parquet_path = dir.path().join("test_data.parquet");
    create_test_parquet(&parquet_path);
    ozzy()
        .current_dir(dir.path())
        .args([
            "data",
            "add",
            parquet_path.to_str().unwrap(),
            "--name",
            "raw",
        ])
        .assert()
        .success();

    let transform_path = dir.path().join("transforms/qc.py");
    fs::create_dir_all(transform_path.parent().unwrap()).unwrap();
    create_test_transform(&transform_path);
    ozzy()
        .current_dir(dir.path())
        .args(["transform", "add", "transforms/qc.py"])
        .assert()
        .success();

    ozzy()
        .current_dir(dir.path())
        .args([
            "endpoint",
            "create",
            "filtered",
            "--input",
            "raw",
            "--transforms",
            "filter_by_value",
        ])
        .assert()
        .success();

    ozzy()
        .current_dir(dir.path())
        .args(["dag", "--format", "svg"])
        .assert()
        .success()
        .stdout(predicate::str::contains("<svg"))
        .stdout(predicate::str::contains("Endpoint: filtered"));
}

#[test]
fn test_scoped_cli_params_apply_to_transform() {
    let dir = tempdir().unwrap();

    ozzy()
        .current_dir(dir.path())
        .args(["init", "--name", "test-project", "--owner", "testuser"])
        .assert()
        .success();

    let parquet_path = dir.path().join("test_data.parquet");
    create_test_parquet(&parquet_path);
    ozzy()
        .current_dir(dir.path())
        .args([
            "data",
            "add",
            parquet_path.to_str().unwrap(),
            "--name",
            "raw",
        ])
        .assert()
        .success();

    let transform_path = dir.path().join("transforms/qc.py");
    fs::create_dir_all(transform_path.parent().unwrap()).unwrap();
    create_test_transform(&transform_path);
    ozzy()
        .current_dir(dir.path())
        .args(["transform", "add", "transforms/qc.py"])
        .assert()
        .success();

    ozzy()
        .current_dir(dir.path())
        .args([
            "endpoint",
            "create",
            "filtered",
            "--input",
            "raw",
            "--transforms",
            "filter_by_value",
        ])
        .assert()
        .success();

    let output_default = dir.path().join("default.parquet");
    ozzy()
        .current_dir(dir.path())
        .args([
            "run",
            "filtered",
            "--output",
            output_default.to_str().unwrap(),
        ])
        .assert()
        .success();

    let output_scoped = dir.path().join("scoped.parquet");
    ozzy()
        .current_dir(dir.path())
        .args([
            "run",
            "filtered",
            "--param",
            "filter_by_value.threshold=15.0",
            "--output",
            output_scoped.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert_eq!(parquet_row_count(&output_default), 4);
    assert_eq!(parquet_row_count(&output_scoped), 2);
}

#[test]
fn test_sap_flux_end_to_end_pipeline() {
    let dir = tempdir().unwrap();

    ozzy()
        .current_dir(dir.path())
        .args(["init", "--name", "sapflux", "--owner", "testuser"])
        .assert()
        .success();

    let parquet_path = dir.path().join("sap_flux_raw.parquet");
    create_sap_flux_parquet(&parquet_path);
    ozzy()
        .current_dir(dir.path())
        .args([
            "data",
            "add",
            parquet_path.to_str().unwrap(),
            "--name",
            "raw",
        ])
        .assert()
        .success();

    let transform_path = dir.path().join("transforms/sap_flux.py");
    fs::create_dir_all(transform_path.parent().unwrap()).unwrap();
    create_sap_flux_transforms(&transform_path);
    ozzy()
        .current_dir(dir.path())
        .args(["transform", "add", "transforms/sap_flux.py"])
        .assert()
        .success()
        .stdout(predicate::str::contains("qc"))
        .stdout(predicate::str::contains("calibrate"))
        .stdout(predicate::str::contains("corrected_signal"));

    ozzy()
        .current_dir(dir.path())
        .args([
            "endpoint",
            "create",
            "corrected",
            "--input",
            "raw",
            "--transforms",
            "qc,calibrate,corrected_signal",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Created endpoint: corrected"));

    let output_path = dir.path().join("corrected.parquet");
    ozzy()
        .current_dir(dir.path())
        .args([
            "run",
            "corrected",
            "--param",
            "qc.min_raw_mv=9.5",
            "--output",
            output_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(
        output_path.exists(),
        "Corrected output parquet should exist"
    );

    ozzy()
        .current_dir(dir.path())
        .args(["commit", "-m", "sap flux e2e fixture"])
        .assert()
        .success();

    ozzy()
        .current_dir(dir.path())
        .args(["log"])
        .assert()
        .success()
        .stdout(predicate::str::contains("sap flux e2e fixture"));
}

fn extract_materialized_hash_prefix(stdout: &str) -> Option<String> {
    stdout
        .lines()
        .find(|line| line.contains("Materialized hash: "))
        .and_then(|line| line.split("Materialized hash: ").nth(1))
        .map(|part| part.trim().trim_end_matches("...").to_string())
}

#[test]
fn test_cache_invalidation_when_transform_code_changes() {
    let dir = tempdir().unwrap();
    let cache_dir = tempdir().unwrap();

    ozzy()
        .current_dir(dir.path())
        .args([
            "init",
            "--name",
            "invalidate-project",
            "--owner",
            "testuser",
        ])
        .assert()
        .success();

    let parquet_path = dir.path().join("test_data.parquet");
    create_test_parquet(&parquet_path);
    ozzy()
        .current_dir(dir.path())
        .args([
            "data",
            "add",
            parquet_path.to_str().unwrap(),
            "--name",
            "raw",
        ])
        .assert()
        .success();

    let transform_path = dir.path().join("transforms/qc.py");
    fs::create_dir_all(transform_path.parent().unwrap()).unwrap();
    create_test_transform(&transform_path);
    ozzy()
        .current_dir(dir.path())
        .args(["transform", "add", "transforms/qc.py"])
        .assert()
        .success();

    ozzy()
        .current_dir(dir.path())
        .args([
            "endpoint",
            "create",
            "filtered",
            "--input",
            "raw",
            "--transforms",
            "filter_by_value",
        ])
        .assert()
        .success();

    ozzy()
        .current_dir(dir.path())
        .env("OZZY_CACHE_DIR", cache_dir.path())
        .args(["run", "filtered"])
        .assert()
        .success()
        .stdout(predicate::str::contains("MISS"));

    let original = fs::read_to_string(&transform_path).unwrap();
    let updated = original.replace(
        "threshold = getattr(params, \"threshold\", 10.0)",
        "threshold = getattr(params, \"threshold\", 11.0)",
    );
    fs::write(&transform_path, updated).unwrap();

    ozzy()
        .current_dir(dir.path())
        .env("OZZY_CACHE_DIR", cache_dir.path())
        .args(["run", "filtered"])
        .assert()
        .success()
        .stdout(predicate::str::contains("MISS"));
}

#[test]
fn test_materialized_hash_is_stable_for_same_inputs() {
    let dir = tempdir().unwrap();
    let cache_dir = tempdir().unwrap();

    ozzy()
        .current_dir(dir.path())
        .args(["init", "--name", "hash-project", "--owner", "testuser"])
        .assert()
        .success();

    let parquet_path = dir.path().join("test_data.parquet");
    create_test_parquet(&parquet_path);
    ozzy()
        .current_dir(dir.path())
        .args([
            "data",
            "add",
            parquet_path.to_str().unwrap(),
            "--name",
            "raw",
        ])
        .assert()
        .success();

    let transform_path = dir.path().join("transforms/qc.py");
    fs::create_dir_all(transform_path.parent().unwrap()).unwrap();
    create_test_transform(&transform_path);
    ozzy()
        .current_dir(dir.path())
        .args(["transform", "add", "transforms/qc.py"])
        .assert()
        .success();

    ozzy()
        .current_dir(dir.path())
        .args([
            "endpoint",
            "create",
            "filtered",
            "--input",
            "raw",
            "--transforms",
            "filter_by_value",
        ])
        .assert()
        .success();

    let output_1 = ozzy()
        .current_dir(dir.path())
        .env("OZZY_CACHE_DIR", cache_dir.path())
        .args(["run", "filtered"])
        .output()
        .unwrap();
    assert!(output_1.status.success());
    let stdout_1 = String::from_utf8_lossy(&output_1.stdout);

    let output_2 = ozzy()
        .current_dir(dir.path())
        .env("OZZY_CACHE_DIR", cache_dir.path())
        .args(["run", "filtered"])
        .output()
        .unwrap();
    assert!(output_2.status.success());
    let stdout_2 = String::from_utf8_lossy(&output_2.stdout);

    let hash_1 = extract_materialized_hash_prefix(&stdout_1).expect("hash missing in first run");
    let hash_2 = extract_materialized_hash_prefix(&stdout_2).expect("hash missing in second run");
    assert_eq!(hash_1, hash_2);
}

#[test]
fn test_endpoint_create_rejects_schema_mismatch() {
    let dir = tempdir().unwrap();

    ozzy()
        .current_dir(dir.path())
        .args(["init", "--name", "schema-fail", "--owner", "testuser"])
        .assert()
        .success();

    let parquet_path = dir.path().join("test_data.parquet");
    create_test_parquet(&parquet_path);
    ozzy()
        .current_dir(dir.path())
        .args([
            "data",
            "add",
            parquet_path.to_str().unwrap(),
            "--name",
            "raw",
        ])
        .assert()
        .success();

    let transform_path = dir.path().join("transforms/bad_schema.py");
    fs::create_dir_all(transform_path.parent().unwrap()).unwrap();
    fs::write(
        &transform_path,
        r#"
import polars as pl

class ozzy:
    @staticmethod
    def transform(**kwargs):
        def decorator(func):
            return func
        return decorator

@ozzy.transform(input_schema={"requires": ["missing_column"]})
def bad_transform(inputs, params):
    return inputs["main"]
"#,
    )
    .unwrap();

    ozzy()
        .current_dir(dir.path())
        .args(["transform", "add", "transforms/bad_schema.py"])
        .assert()
        .success();

    ozzy()
        .current_dir(dir.path())
        .args([
            "endpoint",
            "create",
            "bad",
            "--input",
            "raw",
            "--transforms",
            "bad_transform",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Pipeline validation failed"));
}

#[test]
fn test_dag_show_subcommand_works() {
    let dir = tempdir().unwrap();

    ozzy()
        .current_dir(dir.path())
        .args(["init", "--name", "dag-show", "--owner", "testuser"])
        .assert()
        .success();

    let parquet_path = dir.path().join("test_data.parquet");
    create_test_parquet(&parquet_path);
    ozzy()
        .current_dir(dir.path())
        .args([
            "data",
            "add",
            parquet_path.to_str().unwrap(),
            "--name",
            "raw",
        ])
        .assert()
        .success();

    let transform_path = dir.path().join("transforms/qc.py");
    fs::create_dir_all(transform_path.parent().unwrap()).unwrap();
    create_test_transform(&transform_path);
    ozzy()
        .current_dir(dir.path())
        .args(["transform", "add", "transforms/qc.py"])
        .assert()
        .success();

    ozzy()
        .current_dir(dir.path())
        .args([
            "endpoint",
            "create",
            "filtered",
            "--input",
            "raw",
            "--transforms",
            "filter_by_value",
        ])
        .assert()
        .success();

    ozzy()
        .current_dir(dir.path())
        .args(["dag", "show", "--format", "ascii"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Endpoint: filtered"));
}

#[test]
fn test_staged_endpoint_deletion_blocks_run_and_dag() {
    let dir = tempdir().unwrap();

    ozzy()
        .current_dir(dir.path())
        .args(["init", "--name", "endpoint-delete", "--owner", "testuser"])
        .assert()
        .success();

    let parquet_path = dir.path().join("test_data.parquet");
    create_test_parquet(&parquet_path);
    ozzy()
        .current_dir(dir.path())
        .args([
            "data",
            "add",
            parquet_path.to_str().unwrap(),
            "--name",
            "raw",
        ])
        .assert()
        .success();

    let transform_path = dir.path().join("transforms/qc.py");
    fs::create_dir_all(transform_path.parent().unwrap()).unwrap();
    create_test_transform(&transform_path);
    ozzy()
        .current_dir(dir.path())
        .args(["transform", "add", "transforms/qc.py"])
        .assert()
        .success();

    ozzy()
        .current_dir(dir.path())
        .args([
            "endpoint",
            "create",
            "filtered",
            "--input",
            "raw",
            "--transforms",
            "filter_by_value",
        ])
        .assert()
        .success();

    ozzy()
        .current_dir(dir.path())
        .args(["commit", "-m", "seed"])
        .assert()
        .success();

    ozzy()
        .current_dir(dir.path())
        .args(["endpoint", "rm", "filtered"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Staged endpoint deletion"));

    ozzy()
        .current_dir(dir.path())
        .args(["run", "filtered"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("staged for deletion"));

    ozzy()
        .current_dir(dir.path())
        .args(["dag", "show", "filtered"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn test_transform_add_name_registers_renamed_transform() {
    let dir = tempdir().unwrap();

    ozzy()
        .current_dir(dir.path())
        .args(["init", "--name", "rename-transform", "--owner", "testuser"])
        .assert()
        .success();

    let transform_path = dir.path().join("single.py");
    fs::write(
        &transform_path,
        r#"
import polars as pl

class ozzy:
    @staticmethod
    def transform(**kwargs):
        def decorator(func):
            return func
        return decorator

@ozzy.transform()
def original_name(inputs, params):
    return inputs["main"]
"#,
    )
    .unwrap();

    ozzy()
        .current_dir(dir.path())
        .args([
            "transform",
            "add",
            "single.py",
            "--name",
            "renamed_transform",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("renamed_transform"));

    assert!(dir.path().join("transforms/renamed_transform.py").exists());

    ozzy()
        .current_dir(dir.path())
        .args(["transform", "ls"])
        .assert()
        .success()
        .stdout(predicate::str::contains("renamed_transform"));
}

#[test]
fn test_tag_shorthand_create_works() {
    let dir = tempdir().unwrap();

    ozzy()
        .current_dir(dir.path())
        .args(["init", "--name", "tag-short", "--owner", "testuser"])
        .assert()
        .success();

    let parquet_path = dir.path().join("test_data.parquet");
    create_test_parquet(&parquet_path);
    ozzy()
        .current_dir(dir.path())
        .args([
            "data",
            "add",
            parquet_path.to_str().unwrap(),
            "--name",
            "raw",
        ])
        .assert()
        .success();

    ozzy()
        .current_dir(dir.path())
        .args(["commit", "-m", "seed commit"])
        .assert()
        .success();

    ozzy()
        .current_dir(dir.path())
        .args(["tag", "v1.0.0"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Created tag 'v1.0.0'"));
}

#[test]
fn test_commit_applies_staged_deletions_and_additions() {
    let dir = tempdir().unwrap();

    ozzy()
        .current_dir(dir.path())
        .args(["init", "--name", "staged-commit", "--owner", "testuser"])
        .assert()
        .success();

    let parquet_path = dir.path().join("test_data.parquet");
    create_test_parquet(&parquet_path);
    ozzy()
        .current_dir(dir.path())
        .args([
            "data",
            "add",
            parquet_path.to_str().unwrap(),
            "--name",
            "raw",
        ])
        .assert()
        .success();

    let transform_path = dir.path().join("transforms/qc.py");
    fs::create_dir_all(transform_path.parent().unwrap()).unwrap();
    create_test_transform(&transform_path);
    ozzy()
        .current_dir(dir.path())
        .args(["transform", "add", "transforms/qc.py"])
        .assert()
        .success();

    // Create first endpoint and commit
    ozzy()
        .current_dir(dir.path())
        .args([
            "endpoint",
            "create",
            "old_ep",
            "--input",
            "raw",
            "--transforms",
            "filter_by_value",
        ])
        .assert()
        .success();

    ozzy()
        .current_dir(dir.path())
        .args(["commit", "-m", "initial with old_ep"])
        .assert()
        .success();

    // Stage a deletion of old_ep AND create a new endpoint in the same commit
    ozzy()
        .current_dir(dir.path())
        .args(["endpoint", "rm", "old_ep"])
        .assert()
        .success();

    ozzy()
        .current_dir(dir.path())
        .args([
            "endpoint",
            "create",
            "new_ep",
            "--input",
            "raw",
            "--transforms",
            "filter_by_value",
        ])
        .assert()
        .success();

    ozzy()
        .current_dir(dir.path())
        .args(["commit", "-m", "swap endpoints"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Endpoints: 1"));

    // Verify: new_ep exists, old_ep does not
    ozzy()
        .current_dir(dir.path())
        .args(["endpoint", "ls"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("new_ep")
                .and(predicate::str::contains("old_ep").not()),
        );
}

#[test]
fn test_endpoint_rm_of_staged_override_marks_committed_for_deletion() {
    let dir = tempdir().unwrap();

    ozzy()
        .current_dir(dir.path())
        .args(["init", "--name", "rm-test", "--owner", "testuser"])
        .assert()
        .success();

    create_test_parquet(&dir.path().join("data/raw.parquet"));
    create_test_transform(&dir.path().join("transforms/qc.py"));

    // Create and commit an endpoint
    ozzy()
        .current_dir(dir.path())
        .args([
            "endpoint", "create", "ep1", "--input", "raw", "--transforms", "filter_by_value",
        ])
        .assert()
        .success();

    ozzy()
        .current_dir(dir.path())
        .args(["commit", "-m", "initial"])
        .assert()
        .success();

    // Now create a staged override of the same endpoint
    ozzy()
        .current_dir(dir.path())
        .args([
            "endpoint", "create", "ep1", "--input", "raw", "--transforms", "filter_by_value",
        ])
        .assert()
        .success();

    // Remove the staged override -- should also mark the committed version for deletion
    ozzy()
        .current_dir(dir.path())
        .args(["endpoint", "rm", "ep1"])
        .assert()
        .success();

    // Commit should finalize the deletion
    ozzy()
        .current_dir(dir.path())
        .args(["commit", "-m", "remove ep1"])
        .assert()
        .success();

    // Endpoint should be gone
    ozzy()
        .current_dir(dir.path())
        .args(["endpoint", "ls"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ep1").not());
}
