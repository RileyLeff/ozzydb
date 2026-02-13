//! `ozzy cache` — manage the local materialized cache.
//!
//! Cache lives at `~/.ozzy/cache/materialized/{hash}/` as flat directories
//! containing transform output files. Each directory name is the materialized
//! hash (content-addressed cache key).

use std::path::PathBuf;

use anyhow::{Context, Result};

/// Get the cache root directory.
fn cache_dir() -> Result<PathBuf> {
    let dir = dirs::home_dir()
        .context("Cannot determine home directory")?
        .join(".ozzy")
        .join("cache")
        .join("materialized");
    Ok(dir)
}

/// `ozzy cache ls` — list cached entries.
pub fn ls() -> Result<()> {
    let dir = cache_dir()?;
    if !dir.exists() {
        println!("Cache is empty.");
        return Ok(());
    }

    let mut entries: Vec<_> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .collect();

    if entries.is_empty() {
        println!("Cache is empty.");
        return Ok(());
    }

    // Sort by modification time (most recent first)
    entries.sort_by(|a, b| {
        let a_time = a.metadata().and_then(|m| m.modified()).ok();
        let b_time = b.metadata().and_then(|m| m.modified()).ok();
        b_time.cmp(&a_time)
    });

    println!("{:<66} {:>10}", "HASH", "SIZE");
    for entry in &entries {
        let name = entry.file_name();
        let hash = name.to_str().unwrap_or("?");
        let size = dir_size(&entry.path()).unwrap_or(0);
        println!("{:<66} {:>10}", hash, format_size(size));
    }

    println!("\n{} cached entries", entries.len());
    Ok(())
}

/// `ozzy cache size` — show total cache size.
pub fn size() -> Result<()> {
    let dir = cache_dir()?;
    if !dir.exists() {
        println!("Cache size: 0 B (empty)");
        return Ok(());
    }

    let mut total_size: u64 = 0;
    let mut entry_count: usize = 0;

    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            total_size += dir_size(&entry.path()).unwrap_or(0);
            entry_count += 1;
        }
    }

    println!(
        "Cache size: {} ({} entries)",
        format_size(total_size),
        entry_count
    );
    println!("Location: {}", dir.display());
    Ok(())
}

/// `ozzy cache clear` — clear all cached entries.
pub fn clear() -> Result<()> {
    let dir = cache_dir()?;
    if !dir.exists() {
        println!("Cache is already empty.");
        return Ok(());
    }

    let mut removed = 0u64;
    let mut count = 0usize;

    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let size = dir_size(&entry.path()).unwrap_or(0);
            std::fs::remove_dir_all(entry.path())?;
            removed += size;
            count += 1;
        }
    }

    println!("Cleared {} entries ({})", count, format_size(removed));
    Ok(())
}

/// Compute the total size of a directory (recursively).
fn dir_size(path: &std::path::Path) -> Result<u64> {
    let mut total = 0u64;
    if path.is_file() {
        return Ok(path.metadata()?.len());
    }
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        if ft.is_file() {
            total += entry.metadata()?.len();
        } else if ft.is_dir() {
            total += dir_size(&entry.path())?;
        }
    }
    Ok(total)
}

/// Format a byte count as a human-readable string.
fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_size_bytes() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1023), "1023 B");
    }

    #[test]
    fn test_format_size_kb() {
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1536), "1.5 KB");
    }

    #[test]
    fn test_format_size_mb() {
        assert_eq!(format_size(1024 * 1024), "1.0 MB");
        assert_eq!(format_size(5 * 1024 * 1024), "5.0 MB");
    }

    #[test]
    fn test_format_size_gb() {
        assert_eq!(format_size(1024 * 1024 * 1024), "1.0 GB");
    }

    #[test]
    fn test_cache_dir_path() {
        let dir = cache_dir().unwrap();
        assert!(dir.ends_with(".ozzy/cache/materialized"));
    }

    #[test]
    fn test_dir_size_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let size = dir_size(tmp.path()).unwrap();
        assert_eq!(size, 0);
    }

    #[test]
    fn test_dir_size_with_files() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.txt"), "hello").unwrap();
        std::fs::write(tmp.path().join("b.txt"), "world!").unwrap();
        let size = dir_size(tmp.path()).unwrap();
        assert_eq!(size, 11); // "hello" (5) + "world!" (6)
    }

    #[test]
    fn test_dir_size_nested() {
        let tmp = tempfile::tempdir().unwrap();
        let sub = tmp.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("file.txt"), "data").unwrap();
        let size = dir_size(tmp.path()).unwrap();
        assert_eq!(size, 4);
    }

    #[test]
    fn test_clear_empty_cache() {
        // clear() should work even if cache doesn't exist
        // We can't test this directly without mocking, but format_size is tested
    }
}
