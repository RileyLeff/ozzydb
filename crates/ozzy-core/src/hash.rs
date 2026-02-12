//! Content-addressed hashing using BLAKE3.
//!
//! All artifacts in OzzyDB are identified by their content hash:
//! - data_atom_hash = blake3(raw_bytes)
//! - collection_hash = blake3(sorted member hashes)
//! - transform_hash = blake3(source_hash + function_name + lockfile_hash + environment_image_hash + params_schema_hash)
//! - secrets_hash = blake3(sorted(secret_name + version_id) pairs)
//! - materialized_hash = blake3(sorted(input_name, input_hash) + transform_hash + params_hash + platform_hash + [secrets_hash])

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

/// Compute the BLAKE3 hash of multiple components combined (NUL-separated).
pub fn blake3_hash_components(components: &[&str]) -> String {
    let combined = components.join("\0");
    blake3_hash(combined.as_bytes())
}

/// Compute the hash of a transform (v2).
///
/// transform_hash = blake3(source_hash + function_name + lockfile_hash + environment_image_hash + params_schema_hash)
pub fn transform_hash(
    source_hash: &str,
    function_name: &str,
    lockfile_hash: &str,
    environment_image_hash: &str,
    params_schema_hash: &str,
) -> String {
    blake3_hash_components(&[
        source_hash,
        function_name,
        lockfile_hash,
        environment_image_hash,
        params_schema_hash,
    ])
}

/// Compute the secrets hash for a transform execution.
///
/// secrets_hash = blake3(sorted(secret_name + version_id) pairs)
///
/// Returns `None` if the secrets list is empty (transform declares no secrets).
/// Secret values are never part of the hash — only names and version IDs.
/// When a secret is rotated, its version_id changes → new materialized hash → cache miss.
pub fn secrets_hash(secrets: &[(&str, &str)]) -> Option<String> {
    if secrets.is_empty() {
        return None;
    }

    let mut sorted: Vec<_> = secrets.to_vec();
    sorted.sort_by_key(|(name, _)| *name);

    let combined: String = sorted
        .iter()
        .flat_map(|(name, version_id)| [*name, *version_id])
        .collect::<Vec<_>>()
        .join("\0");

    Some(blake3_hash(combined.as_bytes()))
}

/// Compute the materialized hash (cache key) for a transform execution.
///
/// materialized_hash = blake3(
///   sorted(input_name, input_hash) pairs +
///   transform_hash +
///   params_hash +
///   platform_hash +
///   secrets_hash           // only if transform declares secrets
/// )
pub fn materialized_hash(
    inputs: &[(&str, &str)], // (input_name, input_hash) pairs
    transform_hash: &str,
    params_hash: &str,
    platform_hash: &str,
    secrets_hash: Option<&str>,
) -> String {
    // Sort inputs by name for determinism
    let mut sorted_inputs: Vec<_> = inputs.to_vec();
    sorted_inputs.sort_by_key(|(name, _)| *name);

    // Single-pass hash over all components per spec:
    // blake3(input_name1 \0 input_hash1 \0 ... \0 transform_hash \0 params_hash \0 platform_hash [\0 secrets_hash])
    let mut parts: Vec<&str> = Vec::new();
    for (name, hash) in &sorted_inputs {
        parts.push(name);
        parts.push(hash);
    }
    parts.push(transform_hash);
    parts.push(params_hash);
    parts.push(platform_hash);
    if let Some(sh) = secrets_hash {
        parts.push(sh);
    }
    blake3_hash_components(&parts)
}

/// Compute the hash of a collection version.
///
/// collection_hash = blake3(sorted member reference hashes)
///
/// Member hashes are pre-resolved before calling this function:
/// - Data atoms: the atom's blake3 content hash
/// - Endpoint outputs: the materialized hash of the endpoint
/// - Sub-collections: the sub-collection's version hash
pub fn collection_hash(member_hashes: &[&str]) -> String {
    let mut sorted: Vec<_> = member_hashes.to_vec();
    sorted.sort();
    sorted.dedup(); // set semantics: duplicates don't change the hash
    let combined = sorted.join("\0");
    blake3_hash(combined.as_bytes())
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
    fn test_blake3_hash_length() {
        let hash = blake3_hash(b"test");
        assert_eq!(hash.len(), 64); // BLAKE3 = 256-bit = 64 hex chars
    }

    // -- transform_hash --

    #[test]
    fn test_transform_hash_stable() {
        let h1 = transform_hash("src_abc", "my_func", "lock_def", "env_ghi", "params_jkl");
        let h2 = transform_hash("src_abc", "my_func", "lock_def", "env_ghi", "params_jkl");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn test_transform_hash_changes_with_environment() {
        let h1 = transform_hash("src", "func", "lock", "env_v1", "params");
        let h2 = transform_hash("src", "func", "lock", "env_v2", "params");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_transform_hash_changes_with_source() {
        let h1 = transform_hash("src_v1", "func", "lock", "env", "params");
        let h2 = transform_hash("src_v2", "func", "lock", "env", "params");
        assert_ne!(h1, h2);
    }

    // -- secrets_hash --

    #[test]
    fn test_secrets_hash_empty() {
        assert_eq!(secrets_hash(&[]), None);
    }

    #[test]
    fn test_secrets_hash_single() {
        let h = secrets_hash(&[("API_KEY", "uuid-1")]);
        assert!(h.is_some());
        assert_eq!(h.unwrap().len(), 64);
    }

    #[test]
    fn test_secrets_hash_order_independent() {
        let h1 = secrets_hash(&[("A", "v1"), ("B", "v2")]);
        let h2 = secrets_hash(&[("B", "v2"), ("A", "v1")]);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_secrets_hash_version_change() {
        let h1 = secrets_hash(&[("API_KEY", "uuid-1")]);
        let h2 = secrets_hash(&[("API_KEY", "uuid-2")]);
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_secrets_hash_name_matters() {
        let h1 = secrets_hash(&[("KEY_A", "uuid-1")]);
        let h2 = secrets_hash(&[("KEY_B", "uuid-1")]);
        assert_ne!(h1, h2);
    }

    // -- materialized_hash --

    #[test]
    fn test_materialized_hash_stable() {
        let inputs = [("x", "hash_x")];
        let h1 = materialized_hash(&inputs, "transform", "params", "platform", None);
        let h2 = materialized_hash(&inputs, "transform", "params", "platform", None);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_materialized_hash_input_order_independent() {
        let inputs1 = [("left", "hash_a"), ("right", "hash_b")];
        let inputs2 = [("right", "hash_b"), ("left", "hash_a")];

        let h1 = materialized_hash(&inputs1, "transform", "params", "platform", None);
        let h2 = materialized_hash(&inputs2, "transform", "params", "platform", None);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_materialized_hash_with_secrets() {
        let inputs = [("x", "hash_x")];
        let h_no_secrets = materialized_hash(&inputs, "t", "p", "plat", None);
        let h_with_secrets = materialized_hash(&inputs, "t", "p", "plat", Some("secrets_abc"));
        assert_ne!(h_no_secrets, h_with_secrets);
    }

    #[test]
    fn test_materialized_hash_secrets_change() {
        let inputs = [("x", "hash_x")];
        let h1 = materialized_hash(&inputs, "t", "p", "plat", Some("secrets_v1"));
        let h2 = materialized_hash(&inputs, "t", "p", "plat", Some("secrets_v2"));
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_materialized_hash_different_inputs() {
        let inputs1 = [("x", "hash_a")];
        let inputs2 = [("x", "hash_b")];
        let h1 = materialized_hash(&inputs1, "t", "p", "plat", None);
        let h2 = materialized_hash(&inputs2, "t", "p", "plat", None);
        assert_ne!(h1, h2);
    }

    // -- collection_hash --

    #[test]
    fn test_collection_hash_stable() {
        let h1 = collection_hash(&["hash_a", "hash_b"]);
        let h2 = collection_hash(&["hash_a", "hash_b"]);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_collection_hash_order_independent() {
        let h1 = collection_hash(&["hash_a", "hash_b", "hash_c"]);
        let h2 = collection_hash(&["hash_c", "hash_a", "hash_b"]);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_collection_hash_empty() {
        let h = collection_hash(&[]);
        assert_eq!(h.len(), 64); // valid hash even for empty collection
    }

    #[test]
    fn test_collection_hash_different_members() {
        let h1 = collection_hash(&["hash_a", "hash_b"]);
        let h2 = collection_hash(&["hash_a", "hash_c"]);
        assert_ne!(h1, h2);
    }

    // -- golden value tests --

    #[test]
    fn test_golden_blake3_hash() {
        // blake3("hello") is a known value — verify we produce correct hex
        let h = blake3_hash(b"hello");
        assert_eq!(
            h,
            "ea8f163db38682925e4491c5e58d4bb3506ef8c14eb78a86e908c5624a67200f"
        );
    }

    #[test]
    fn test_golden_transform_hash() {
        // Pin a specific transform hash so we detect accidental changes to the formula
        let h = transform_hash("src", "func", "lock", "env", "params");
        // This is blake3("src\0func\0lock\0env\0params")
        let expected = blake3_hash(b"src\0func\0lock\0env\0params");
        assert_eq!(h, expected);
    }

    #[test]
    fn test_golden_secrets_hash() {
        // Pin: secrets_hash([("A","v1"),("B","v2")]) = blake3("A\0v1\0B\0v2")
        let h = secrets_hash(&[("A", "v1"), ("B", "v2")]).unwrap();
        let expected = blake3_hash(b"A\0v1\0B\0v2");
        assert_eq!(h, expected);
    }

    #[test]
    fn test_golden_collection_hash() {
        // Pin: collection_hash(["b","a"]) = blake3("a\0b") since sorted
        let h = collection_hash(&["b", "a"]);
        let expected = blake3_hash(b"a\0b");
        assert_eq!(h, expected);
    }

    #[test]
    fn test_golden_materialized_hash() {
        // Single-pass: blake3("x\0hash_x\0t\0p\0plat")
        let inputs = [("x", "hash_x")];
        let h = materialized_hash(&inputs, "t", "p", "plat", None);
        let expected = blake3_hash(b"x\0hash_x\0t\0p\0plat");
        assert_eq!(h, expected);
    }

    #[test]
    fn test_golden_materialized_hash_with_secrets() {
        // Single-pass: blake3("x\0hash_x\0t\0p\0plat\0s")
        let inputs = [("x", "hash_x")];
        let h = materialized_hash(&inputs, "t", "p", "plat", Some("s"));
        let expected = blake3_hash(b"x\0hash_x\0t\0p\0plat\0s");
        assert_eq!(h, expected);
    }

    #[test]
    fn test_collection_hash_dedup() {
        // Duplicate member hashes should not change the result (set semantics)
        let h1 = collection_hash(&["hash_a", "hash_b"]);
        let h2 = collection_hash(&["hash_a", "hash_b", "hash_a"]);
        assert_eq!(h1, h2);
    }
}
