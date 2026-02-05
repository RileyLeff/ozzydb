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
        .args(["run", "--with", "polars", "--with", "pyarrow", "python", "-c", &script])
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
        .args(["data", "add", parquet_path.to_str().unwrap(), "--name", "raw"])
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
        .args(["data", "add", parquet_path.to_str().unwrap(), "--name", "raw"])
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
        .args(["endpoint", "create", "filtered", "--input", "raw", "--transforms", "filter_by_value"])
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
        .args(["data", "add", parquet_path.to_str().unwrap(), "--name", "raw"])
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
        .stdout(predicate::str::contains("clean").or(predicate::str::contains("Nothing to commit")));
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
        .args(["endpoint", "create", "test", "--input", "nonexistent", "--transforms", "qc"])
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
        .args(["data", "add", parquet_path.to_str().unwrap(), "--name", "raw"])
        .assert()
        .success();

    // Try to create endpoint with non-existent transform
    ozzy()
        .current_dir(dir.path())
        .args(["endpoint", "create", "test", "--input", "raw", "--transforms", "nonexistent"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn test_caching_works() {
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
        .args(["data", "add", parquet_path.to_str().unwrap(), "--name", "raw"])
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
        .args(["endpoint", "create", "filtered", "--input", "raw", "--transforms", "filter_by_value"])
        .assert()
        .success();

    // First run - should be a cache MISS
    ozzy()
        .current_dir(dir.path())
        .args(["run", "filtered"])
        .assert()
        .success()
        .stdout(predicate::str::contains("MISS"));

    // Second run - should be a cache HIT
    ozzy()
        .current_dir(dir.path())
        .args(["run", "filtered"])
        .assert()
        .success()
        .stdout(predicate::str::contains("HIT"));

    // Run with --force should skip cache
    ozzy()
        .current_dir(dir.path())
        .args(["run", "filtered", "--force"])
        .assert()
        .success()
        .stdout(predicate::str::contains("SKIP"));
}
