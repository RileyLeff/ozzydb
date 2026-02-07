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
}
