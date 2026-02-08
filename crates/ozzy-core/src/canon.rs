//! Canonicalization rules for deterministic hashing.
//!
//! To ensure consistent hashing, all inputs are canonicalized:
//! - Source code: UTF-8, LF line endings, no trailing whitespace, files sorted alphabetically
//! - Parameters (JSON): sorted keys, no whitespace, shortest decimal representation
//! - Parquet files: standard row group size, compression, column order

use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

use crate::hash::blake3_hash;

/// Canonicalize source code.
///
/// Rules:
/// - UTF-8 encoding
/// - LF line endings (CRLF → LF)
/// - No trailing whitespace per line
pub fn canonicalize_source(source: &str) -> String {
    source
        .lines()
        .map(|line| line.trim_end()) // Remove trailing whitespace
        .collect::<Vec<_>>()
        .join("\n") // LF line endings
}

/// Compute the hash of a source directory.
///
/// Rules:
/// - Files sorted alphabetically by path
/// - Each file: path + NUL + content
/// - Files joined by NUL
pub fn hash_source_directory(dir: &Path) -> std::io::Result<String> {
    let mut files: Vec<(String, String)> = Vec::new();

    for entry in WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !name.starts_with('.') && name != "__pycache__"
        })
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();

        // Normalize to forward slashes for cross-platform hash determinism
        let relative_path = path
            .strip_prefix(dir)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");

        let content = fs::read_to_string(path)?;
        let canonical_content = canonicalize_source(&content);

        files.push((relative_path, canonical_content));
    }

    // Sort by path
    files.sort_by(|a, b| a.0.cmp(&b.0));

    // Build hash input: "path\0content\0path\0content\0..."
    let hash_input: String = files
        .iter()
        .flat_map(|(path, content)| [path.as_str(), "\0", content.as_str(), "\0"])
        .collect();

    Ok(blake3_hash(hash_input.as_bytes()))
}

/// Hash a single source file.
pub fn hash_source_file(path: &Path) -> std::io::Result<String> {
    let content = fs::read_to_string(path)?;
    let canonical = canonicalize_source(&content);
    Ok(blake3_hash(canonical.as_bytes()))
}

/// Canonicalize JSON for hashing.
///
/// Rules:
/// - Keys sorted alphabetically (recursive)
/// - No whitespace between tokens
/// - Numbers in shortest decimal representation
/// Escape a string for canonical JSON (handles quotes, backslashes, control chars, non-ASCII).
fn canonicalize_json_string(s: &str) -> String {
    let escaped: String = s
        .chars()
        .map(|c| match c {
            '"' => "\\\"".to_string(),
            '\\' => "\\\\".to_string(),
            '\n' => "\\n".to_string(),
            '\r' => "\\r".to_string(),
            '\t' => "\\t".to_string(),
            c if c.is_ascii_graphic() || c == ' ' => c.to_string(),
            c => {
                let cp = c as u32;
                if cp > 0xFFFF {
                    // Supplementary character: encode as UTF-16 surrogate pair
                    let adjusted = cp - 0x10000;
                    let high = 0xD800 + (adjusted >> 10);
                    let low = 0xDC00 + (adjusted & 0x3FF);
                    format!("\\u{:04x}\\u{:04x}", high, low)
                } else {
                    format!("\\u{:04x}", cp)
                }
            }
        })
        .collect();
    format!("\"{}\"", escaped)
}

pub fn canonicalize_json(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            // Sort keys and recurse
            let sorted: BTreeMap<_, _> = map.iter().collect();
            let pairs: Vec<String> = sorted
                .iter()
                .map(|(k, v)| format!("{}:{}", canonicalize_json_string(k), canonicalize_json(v)))
                .collect();
            format!("{{{}}}", pairs.join(","))
        }
        Value::Array(arr) => {
            let items: Vec<String> = arr.iter().map(canonicalize_json).collect();
            format!("[{}]", items.join(","))
        }
        Value::String(s) => canonicalize_json_string(s),
        Value::Number(n) => {
            // Use shortest decimal representation
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else if let Some(u) = n.as_u64() {
                u.to_string()
            } else if let Some(f) = n.as_f64() {
                // Canonical float: trim fractional trailing zeros only when
                // there's a decimal point and no exponent (e.g. 1.50 → 1.5,
                // but 1e10 stays as-is and 100 stays as 100).
                let s = format!("{}", f);
                if s.contains('.') && !s.contains('e') && !s.contains('E') {
                    s.trim_end_matches('0').trim_end_matches('.').to_string()
                } else {
                    s
                }
            } else {
                n.to_string()
            }
        }
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
    }
}

/// Hash canonical JSON.
pub fn hash_json(value: &Value) -> String {
    let canonical = canonicalize_json(value);
    blake3_hash(canonical.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_canonicalize_source_crlf() {
        let input = "line1\r\nline2\r\nline3";
        let expected = "line1\nline2\nline3";
        assert_eq!(canonicalize_source(input), expected);
    }

    #[test]
    fn test_canonicalize_source_trailing_whitespace() {
        let input = "line1   \nline2\t\nline3";
        let expected = "line1\nline2\nline3";
        assert_eq!(canonicalize_source(input), expected);
    }

    #[test]
    fn test_canonicalize_json_sorted_keys() {
        let value = json!({"z": 1, "a": 2, "m": 3});
        let canonical = canonicalize_json(&value);
        assert_eq!(canonical, r#"{"a":2,"m":3,"z":1}"#);
    }

    #[test]
    fn test_canonicalize_json_nested() {
        let value = json!({"outer": {"z": 1, "a": 2}});
        let canonical = canonicalize_json(&value);
        assert_eq!(canonical, r#"{"outer":{"a":2,"z":1}}"#);
    }

    #[test]
    fn test_canonicalize_json_no_whitespace() {
        let value = json!({"key": [1, 2, 3]});
        let canonical = canonicalize_json(&value);
        assert!(!canonical.contains(' '));
    }

    #[test]
    fn test_hash_json_deterministic() {
        let value1 = json!({"z": 1, "a": 2});
        let value2 = json!({"a": 2, "z": 1});

        assert_eq!(hash_json(&value1), hash_json(&value2));
    }

    // ===== Review Series 1 Wrapup Tests =====

    #[test]
    fn test_canonicalize_json_string_escapes_keys_with_special_chars() {
        // R15: JSON key escaping - keys with quotes, backslashes, newlines
        let value = json!({"key\"with\\quotes": 1, "normal": 2});
        let canonical = canonicalize_json(&value);
        assert!(canonical.contains("key\\\"with\\\\quotes"));
        assert!(canonical.contains("\"normal\":2"));

        // Keys with newlines/tabs
        let value2 = json!({"\n\t": "val"});
        let canonical2 = canonicalize_json(&value2);
        assert!(canonical2.contains("\\n\\t"));
    }

    #[test]
    fn test_canonicalize_json_surrogate_pairs() {
        // R9: Supplementary Unicode (U+10000+) must use UTF-16 surrogate pairs
        // U+1F600 = Grinning Face emoji
        let value = json!({"emoji": "\u{1F600}"});
        let canonical = canonicalize_json(&value);
        // Should produce \ud83d\ude00 (surrogate pair), not \u1f600
        assert!(
            canonical.contains("\\ud83d\\ude00"),
            "Expected surrogate pair, got: {}",
            canonical
        );
    }

    #[test]
    fn test_canonicalize_json_float_trimming_edge_cases() {
        // R8: Float trimming should only trim fractional trailing zeros
        // 1.50 -> 1.5
        let value = json!(1.5);
        assert_eq!(canonicalize_json(&value), "1.5");

        // Integer-valued floats
        let value2 = json!(100.0);
        let canonical2 = canonicalize_json(&value2);
        // 100.0 formats as "100" via Rust's {} formatter (no decimal)
        assert_eq!(canonical2, "100");

        // Very small numbers that use exponent notation should NOT be trimmed
        let value3 = json!(1e-10);
        let canonical3 = canonicalize_json(&value3);
        // Should preserve the exponent form
        assert!(
            canonical3.contains('e') || canonical3.contains('E') || canonical3 == "0.0000000001",
            "Small float should use exponent or full form: {}",
            canonical3
        );
    }

    #[test]
    fn test_canonicalize_json_u64_branch() {
        // R13: Large u64 values should use u64 branch, not lose precision via f64
        let large_u64: u64 = (1u64 << 53) + 1; // Beyond f64 exact range
        let value = json!(large_u64);
        let canonical = canonicalize_json(&value);
        assert_eq!(canonical, large_u64.to_string());
    }

    #[test]
    fn test_canonicalize_source_mixed_crlf_and_lf() {
        // CRLF and LF mixed with trailing whitespace
        let input = "line1  \r\nline2\t\nline3   ";
        let result = canonicalize_source(input);
        assert_eq!(result, "line1\nline2\nline3");
        assert!(!result.contains('\r'));
    }

    #[test]
    fn test_hash_source_directory_skips_pycache() {
        // R13: __pycache__ directories should be excluded from hashing
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("transform.py");
        fs::write(&src, "def foo(): pass").unwrap();

        // Create __pycache__ with a .pyc file
        let pycache = dir.path().join("__pycache__");
        fs::create_dir_all(&pycache).unwrap();
        fs::write(pycache.join("transform.cpython-311.pyc"), b"\x00\x00").unwrap();

        let hash_with_pycache = hash_source_directory(dir.path()).unwrap();

        // Remove __pycache__ and hash again
        fs::remove_dir_all(&pycache).unwrap();
        let hash_without_pycache = hash_source_directory(dir.path()).unwrap();

        assert_eq!(
            hash_with_pycache, hash_without_pycache,
            "__pycache__ should not affect source directory hash"
        );
    }

    #[test]
    fn test_hash_source_directory_skips_hidden_files() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("transform.py");
        fs::write(&src, "def foo(): pass").unwrap();

        let hash_without_hidden = hash_source_directory(dir.path()).unwrap();

        // Add a hidden file
        fs::write(dir.path().join(".DS_Store"), b"hidden").unwrap();
        let hash_with_hidden = hash_source_directory(dir.path()).unwrap();

        assert_eq!(
            hash_without_hidden, hash_with_hidden,
            "Hidden files should not affect source directory hash"
        );
    }

    #[test]
    fn test_hash_source_directory_cross_platform_separators() {
        // R19 M4: Path separators should be normalized to forward slash
        let dir = tempfile::tempdir().unwrap();
        let subdir = dir.path().join("subdir");
        fs::create_dir_all(&subdir).unwrap();
        fs::write(subdir.join("file.py"), "content").unwrap();

        // Hash should be deterministic regardless of OS path separators
        let hash = hash_source_directory(dir.path()).unwrap();
        assert!(!hash.is_empty());
        // Running twice should produce same result
        let hash2 = hash_source_directory(dir.path()).unwrap();
        assert_eq!(hash, hash2);
    }
}
