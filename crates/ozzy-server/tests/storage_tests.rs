//! Integration tests for R2 storage.
//!
//! These tests require R2/S3-compatible credentials to run.
//! Set the following environment variables:
//! - R2_ENDPOINT
//! - R2_BUCKET (will use a test prefix to avoid conflicts)
//! - R2_ACCESS_KEY_ID
//! - R2_SECRET_ACCESS_KEY
//!
//! To run against local MinIO:
//! ```bash
//! docker run -p 9000:9000 -p 9001:9001 minio/minio server /data --console-address ":9001"
//! export R2_ENDPOINT=http://localhost:9000
//! export R2_BUCKET=test
//! export R2_ACCESS_KEY_ID=minioadmin
//! export R2_SECRET_ACCESS_KEY=minioadmin
//! cargo test --package ozzy-server storage_tests
//! ```

use anyhow::Result;
use ozzy_server::config::R2Config;
use ozzy_server::storage::ContentStorage;
use std::env;

fn get_test_config() -> Option<R2Config> {
    // Check if R2 credentials are available
    let endpoint = env::var("R2_ENDPOINT").ok()?;
    let bucket = env::var("R2_BUCKET").ok()?;
    let access_key_id = env::var("R2_ACCESS_KEY_ID").ok()?;
    let secret_access_key = env::var("R2_SECRET_ACCESS_KEY").ok()?;

    Some(R2Config {
        endpoint,
        bucket,
        access_key_id,
        secret_access_key,
        region: env::var("R2_REGION").unwrap_or_else(|_| "auto".into()),
    })
}

fn should_skip_r2_tests() -> bool {
    get_test_config().is_none()
}

/// Generate a unique test prefix to avoid conflicts between test runs.
fn test_prefix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("test-{}", timestamp)
}

#[tokio::test]
async fn test_store_and_retrieve() -> Result<()> {
    if should_skip_r2_tests() {
        eprintln!("Skipping R2 tests - no credentials configured");
        return Ok(());
    }

    let config = get_test_config().unwrap();
    let storage = ContentStorage::with_prefix(&config, test_prefix())?;

    let content = b"hello world test content";
    let hash = storage.store(content, "txt").await?;

    // Verify content exists
    assert!(storage.exists(&hash, "txt").await?);

    // Retrieve and compare
    let retrieved = storage.get(&hash, "txt").await?;
    assert_eq!(&retrieved[..], content);

    // Cleanup
    storage.delete(&hash, "txt").await?;
    assert!(!storage.exists(&hash, "txt").await?);

    Ok(())
}

#[tokio::test]
async fn test_deduplication() -> Result<()> {
    if should_skip_r2_tests() {
        eprintln!("Skipping R2 tests - no credentials configured");
        return Ok(());
    }

    let config = get_test_config().unwrap();
    let storage = ContentStorage::with_prefix(&config, test_prefix())?;

    let content = b"duplicate content for dedup test";

    // Store twice
    let hash1 = storage.store(content, "txt").await?;
    let hash2 = storage.store(content, "txt").await?;

    // Hashes should be identical
    assert_eq!(hash1, hash2);

    // Verify content is stored
    assert!(storage.exists(&hash1, "txt").await?);

    // Cleanup
    storage.delete(&hash1, "txt").await?;

    Ok(())
}

#[tokio::test]
async fn test_check_existing_batch() -> Result<()> {
    if should_skip_r2_tests() {
        eprintln!("Skipping R2 tests - no credentials configured");
        return Ok(());
    }

    let config = get_test_config().unwrap();
    let storage = ContentStorage::with_prefix(&config, test_prefix())?;

    // Store some content
    let content1 = b"content one";
    let content2 = b"content two";
    let hash1 = storage.store(content1, "txt").await?;
    let hash2 = storage.store(content2, "txt").await?;

    // Create a fake hash that doesn't exist
    let fake_hash = "0000000000000000000000000000000000000000000000000000000000000000";

    // Check batch
    let hashes_to_check = vec![
        (hash1.clone(), "txt".to_string()),
        (hash2.clone(), "txt".to_string()),
        (fake_hash.to_string(), "txt".to_string()),
    ];

    let existing = storage.check_existing(&hashes_to_check).await?;

    // Should only contain the two real hashes
    assert_eq!(existing.len(), 2);
    assert!(existing.contains(&hash1));
    assert!(existing.contains(&hash2));
    assert!(!existing.contains(&fake_hash.to_string()));

    // Cleanup
    storage.delete(&hash1, "txt").await?;
    storage.delete(&hash2, "txt").await?;

    Ok(())
}

#[tokio::test]
async fn test_parquet_storage() -> Result<()> {
    if should_skip_r2_tests() {
        eprintln!("Skipping R2 tests - no credentials configured");
        return Ok(());
    }

    let config = get_test_config().unwrap();
    let storage = ContentStorage::with_prefix(&config, test_prefix())?;

    // Simulate parquet-like content (just binary data for the test)
    let content = b"PAR1\x00\x00\x00\x00fake parquet header and data";
    let hash = storage.store(content, "parquet").await?;

    // Verify it's stored with correct extension
    assert!(storage.exists(&hash, "parquet").await?);
    // Different extension should not exist
    assert!(!storage.exists(&hash, "txt").await?);

    let retrieved = storage.get(&hash, "parquet").await?;
    assert_eq!(&retrieved[..], content);

    // Cleanup
    storage.delete(&hash, "parquet").await?;

    Ok(())
}

#[tokio::test]
async fn test_python_transform_storage() -> Result<()> {
    if should_skip_r2_tests() {
        eprintln!("Skipping R2 tests - no credentials configured");
        return Ok(());
    }

    let config = get_test_config().unwrap();
    let storage = ContentStorage::with_prefix(&config, test_prefix())?;

    let python_code = br#"
import polars as pl
from ozzy import transform

@transform(
    inputs={"raw": "raw"},
    output_schema={"id": "int64", "value": "float64"}
)
def clean_data(raw: pl.DataFrame) -> pl.DataFrame:
    return raw.select(["id", "value"]).drop_nulls()
"#;

    let hash = storage.store(python_code, "py").await?;

    assert!(storage.exists(&hash, "py").await?);

    let retrieved = storage.get(&hash, "py").await?;
    assert_eq!(&retrieved[..], python_code);

    // Cleanup
    storage.delete(&hash, "py").await?;

    Ok(())
}

#[tokio::test]
async fn test_metadata() -> Result<()> {
    if should_skip_r2_tests() {
        eprintln!("Skipping R2 tests - no credentials configured");
        return Ok(());
    }

    let config = get_test_config().unwrap();
    let storage = ContentStorage::with_prefix(&config, test_prefix())?;

    let content = b"content for metadata test";
    let hash = storage.store(content, "txt").await?;

    // Get metadata
    let meta = storage.metadata(&hash, "txt").await?;

    // Verify size
    assert_eq!(meta.size, content.len());

    // Cleanup
    storage.delete(&hash, "txt").await?;

    Ok(())
}

#[tokio::test]
async fn test_get_nonexistent() -> Result<()> {
    if should_skip_r2_tests() {
        eprintln!("Skipping R2 tests - no credentials configured");
        return Ok(());
    }

    let config = get_test_config().unwrap();
    let storage = ContentStorage::with_prefix(&config, test_prefix())?;

    let fake_hash = "0000000000000000000000000000000000000000000000000000000000000000";

    // Should return error for non-existent content
    let result = storage.get(fake_hash, "txt").await;
    assert!(result.is_err());

    Ok(())
}

#[tokio::test]
async fn test_large_file() -> Result<()> {
    if should_skip_r2_tests() {
        eprintln!("Skipping R2 tests - no credentials configured");
        return Ok(());
    }

    let config = get_test_config().unwrap();
    let storage = ContentStorage::with_prefix(&config, test_prefix())?;

    // Create 1MB of data
    let content: Vec<u8> = (0..1_000_000).map(|i| (i % 256) as u8).collect();
    let hash = storage.store(&content, "bin").await?;

    assert!(storage.exists(&hash, "bin").await?);

    let retrieved = storage.get(&hash, "bin").await?;
    assert_eq!(retrieved.len(), content.len());
    assert_eq!(&retrieved[..], &content[..]);

    // Cleanup
    storage.delete(&hash, "bin").await?;

    Ok(())
}
