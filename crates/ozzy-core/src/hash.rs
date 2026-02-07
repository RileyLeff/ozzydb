//! Content-addressed hashing using BLAKE3.
//!
//! All artifacts in OzzyDB are identified by their content hash:
//! - raw_data_hash = blake3(raw_parquet_bytes)
//! - transform_hash = blake3(canonical_source + lockfile_hash + runtime_version + params_schema_hash)
//! - materialized_hash = blake3(input_hash + transform_hash + canonical_params_hash + platform_fingerprint)

use blake3::Hasher;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

/// Compute the BLAKE3 hash of a byte slice.
pub fn blake3_hash(data: &[u8]) -> String {
    let hash = blake3::hash(data);
    hash.to_hex().to_string()
}

/// Compute the BLAKE3 hash of a file.
pub fn blake3_hash_file(path: &Path) -> std::io::Result<String> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut hasher = Hasher::new();

    let mut buffer = [0u8; 65536]; // 64KB buffer
    loop {
        let bytes_read = reader.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    Ok(hasher.finalize().to_hex().to_string())
}

/// Compute the BLAKE3 hash of multiple components combined.
pub fn blake3_hash_components(components: &[&str]) -> String {
    let combined = components.join("\0");
    blake3_hash(combined.as_bytes())
}

/// Compute the hash of a transform.
///
/// transform_hash = blake3(canonical_source + lockfile_hash + runtime_version + params_schema_hash)
pub fn transform_hash(
    source_hash: &str,
    lockfile_hash: &str,
    runtime_version: &str,
    params_schema_hash: &str,
) -> String {
    blake3_hash_components(&[
        source_hash,
        lockfile_hash,
        runtime_version,
        params_schema_hash,
    ])
}

/// Compute the materialized hash for a transform execution.
///
/// materialized_hash = blake3(input_hash + transform_hash + canonical_params_hash + platform_hash)
pub fn materialized_hash(
    input_hash: &str,
    transform_hash: &str,
    params_hash: &str,
    platform_hash: &str,
) -> String {
    blake3_hash_components(&[input_hash, transform_hash, params_hash, platform_hash])
}

/// Compute the hash of a multi-input transform execution.
///
/// For transforms with multiple inputs, we hash the sorted list of (input_name, input_hash) pairs.
pub fn materialized_hash_multi_input(
    inputs: &[(&str, &str)], // (input_name, input_hash) pairs
    transform_hash: &str,
    params_hash: &str,
    platform_hash: &str,
) -> String {
    // Sort inputs by name for determinism
    let mut sorted_inputs: Vec<_> = inputs.iter().collect();
    sorted_inputs.sort_by_key(|(name, _)| *name);

    // Build input string: "name1\0hash1\0name2\0hash2\0..."
    let input_str: String = sorted_inputs
        .iter()
        .flat_map(|(name, hash)| [*name, *hash])
        .collect::<Vec<_>>()
        .join("\0");

    let input_hash = blake3_hash(input_str.as_bytes());
    blake3_hash_components(&[&input_hash, transform_hash, params_hash, platform_hash])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blake3_hash_deterministic() {
        let data = b"hello world";
        let hash1 = blake3_hash(data);
        let hash2 = blake3_hash(data);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_blake3_hash_different_input() {
        let hash1 = blake3_hash(b"hello");
        let hash2 = blake3_hash(b"world");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_transform_hash() {
        let hash = transform_hash("source123", "lockfile456", "python-3.11", "params789");
        assert!(!hash.is_empty());
        assert_eq!(hash.len(), 64); // BLAKE3 produces 256-bit (64 hex chars) hashes
    }

    #[test]
    fn test_materialized_hash_multi_input_order_independent() {
        let inputs1 = vec![("left", "hash_a"), ("right", "hash_b")];
        let inputs2 = vec![("right", "hash_b"), ("left", "hash_a")];

        let hash1 = materialized_hash_multi_input(&inputs1, "transform", "params", "platform");
        let hash2 = materialized_hash_multi_input(&inputs2, "transform", "params", "platform");

        assert_eq!(hash1, hash2);
    }
}
