//! Local cache for computed transform outputs.
//!
//! The cache is stored at ~/.ozzy/cache/ with a SQLite index.
//! Cache entries are content-addressed by their materialized hash.

use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::Result;

/// Local cache entry metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    pub materialized_hash: String,
    pub platform: String,
    pub row_count: Option<u64>,
    pub byte_size: Option<u64>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_accessed: chrono::DateTime<chrono::Utc>,
    pub access_count: u64,
    pub file_path: PathBuf,
}

impl CacheEntry {
    pub fn file_path(&self) -> &PathBuf {
        &self.file_path
    }
}

/// Local cache manager.
pub struct LocalCache {
    cache_dir: PathBuf,
    db_path: PathBuf,
}

impl LocalCache {
    /// Open or create the local cache.
    pub fn open() -> Result<Self> {
        let cache_dir = default_cache_dir();
        Self::open_at(&cache_dir)
    }

    /// Open or create the local cache at a specific location.
    pub fn open_at(cache_dir: &Path) -> Result<Self> {
        fs::create_dir_all(cache_dir)?;
        fs::create_dir_all(cache_dir.join("data"))?;

        let db_path = cache_dir.join("index.db");
        let cache = Self {
            cache_dir: cache_dir.to_path_buf(),
            db_path,
        };

        cache.init_db()?;
        Ok(cache)
    }

    /// Get the cache directory path.
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// Initialize the SQLite database schema.
    fn init_db(&self) -> Result<()> {
        let conn = self.connect()?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS cache_entries (
                materialized_hash TEXT PRIMARY KEY,
                platform TEXT NOT NULL,
                file_path TEXT NOT NULL,
                row_count INTEGER,
                byte_size INTEGER,
                created_at TEXT NOT NULL,
                last_accessed TEXT NOT NULL,
                access_count INTEGER NOT NULL DEFAULT 1
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_cache_last_accessed ON cache_entries(last_accessed)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_cache_platform ON cache_entries(platform)",
            [],
        )?;

        Ok(())
    }

    fn connect(&self) -> Result<Connection> {
        Ok(Connection::open(&self.db_path)?)
    }

    /// Get the file path for a cached entry.
    pub fn get_path(&self, hash: &str) -> Result<Option<PathBuf>> {
        self.get(hash)
            .map(|e| e.map(|entry| entry.file_path().clone()))
    }

    /// List all cache entries.
    pub fn list(&self) -> Result<Vec<CacheEntry>> {
        let conn = self.connect()?;

        let mut stmt = conn.prepare(
            "SELECT materialized_hash, platform, file_path, row_count, byte_size,
                    created_at, last_accessed, access_count
             FROM cache_entries
             ORDER BY last_accessed DESC",
        )?;

        let entries = stmt
            .query_map([], |row| {
                Ok(CacheEntry {
                    materialized_hash: row.get(0)?,
                    platform: row.get(1)?,
                    row_count: row.get(3)?,
                    byte_size: row.get(4)?,
                    created_at: row
                        .get::<_, String>(5)?
                        .parse()
                        .unwrap_or_else(|_| Utc::now()),
                    last_accessed: row
                        .get::<_, String>(6)?
                        .parse()
                        .unwrap_or_else(|_| Utc::now()),
                    access_count: row.get(7)?,
                    file_path: PathBuf::from(row.get::<_, String>(2)?),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(entries)
    }

    /// Clear all cache entries.
    pub fn clear(&self) -> Result<()> {
        // Remove all files
        let data_dir = self.cache_dir.join("data");
        if data_dir.exists() {
            for entry in fs::read_dir(&data_dir)? {
                let entry = entry?;
                if entry
                    .path()
                    .extension()
                    .map(|e| e == "parquet")
                    .unwrap_or(false)
                {
                    fs::remove_file(entry.path())?;
                }
            }
        }

        // Clear database
        let conn = self.connect()?;
        conn.execute("DELETE FROM cache_entries", [])?;

        Ok(())
    }

    /// Evict oldest entries to reach target size.
    pub fn evict_to_size(&self, target_bytes: u64) -> Result<usize> {
        let mut evicted = 0;

        while self.total_size()? > target_bytes {
            let conn = self.connect()?;

            // Get oldest entry
            let oldest: Option<String> = conn
                .query_row(
                    "SELECT materialized_hash FROM cache_entries ORDER BY last_accessed ASC LIMIT 1",
                    [],
                    |row| row.get(0),
                )
                .optional()?;

            if let Some(hash) = oldest {
                self.remove(&hash)?;
                evicted += 1;
            } else {
                break;
            }
        }

        Ok(evicted)
    }
}

impl LocalCache {
    /// Get a cache entry by materialized hash.
    pub fn get(&self, hash: &str) -> Result<Option<CacheEntry>> {
        let conn = self.connect()?;

        let entry: Option<CacheEntry> = conn
            .query_row(
                "SELECT materialized_hash, platform, file_path, row_count, byte_size,
                        created_at, last_accessed, access_count
                 FROM cache_entries WHERE materialized_hash = ?",
                [hash],
                |row| {
                    Ok(CacheEntry {
                        materialized_hash: row.get(0)?,
                        platform: row.get(1)?,
                        row_count: row.get(3)?,
                        byte_size: row.get(4)?,
                        created_at: row
                            .get::<_, String>(5)?
                            .parse()
                            .unwrap_or_else(|_| Utc::now()),
                        last_accessed: row
                            .get::<_, String>(6)?
                            .parse()
                            .unwrap_or_else(|_| Utc::now()),
                        access_count: row.get(7)?,
                        file_path: PathBuf::from(row.get::<_, String>(2)?),
                    })
                },
            )
            .optional()?;

        // Verify the cached file still exists on disk
        if let Some(ref e) = entry {
            if !e.file_path.exists() {
                // File was externally deleted; clean up the stale DB entry
                conn.execute(
                    "DELETE FROM cache_entries WHERE materialized_hash = ?",
                    [hash],
                )?;
                return Ok(None);
            }
            // Update access stats
            conn.execute(
                "UPDATE cache_entries
                 SET last_accessed = ?, access_count = access_count + 1
                 WHERE materialized_hash = ?",
                params![Utc::now().to_rfc3339(), hash],
            )?;
        }

        Ok(entry)
    }

    /// Check if a hash is in the cache.
    pub fn contains(&self, hash: &str) -> Result<bool> {
        let conn = self.connect()?;

        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM cache_entries WHERE materialized_hash = ?",
            [hash],
            |row| row.get(0),
        )?;

        Ok(count > 0)
    }

    /// Store a cache entry.
    pub fn put(
        &self,
        hash: &str,
        platform: &str,
        source_path: &Path,
        row_count: Option<u64>,
    ) -> Result<PathBuf> {
        let dest_path = self
            .cache_dir
            .join("data")
            .join(format!("{}.parquet", hash));

        // Copy the file to cache
        fs::copy(source_path, &dest_path)?;

        let byte_size = fs::metadata(&dest_path)?.len();
        let now = Utc::now();

        let conn = self.connect()?;
        conn.execute(
            "INSERT OR REPLACE INTO cache_entries
             (materialized_hash, platform, file_path, row_count, byte_size, created_at, last_accessed, access_count)
             VALUES (?, ?, ?, ?, ?, ?, ?, 1)",
            params![
                hash,
                platform,
                dest_path.to_string_lossy(),
                row_count.map(|n| n as i64),
                byte_size as i64,
                now.to_rfc3339(),
                now.to_rfc3339(),
            ],
        )?;

        Ok(dest_path)
    }

    /// Look up a cache entry's file path without updating access statistics.
    fn get_path_only(&self, hash: &str) -> Result<Option<PathBuf>> {
        let conn = self.connect()?;
        let mut stmt =
            conn.prepare("SELECT file_path FROM cache_entries WHERE materialized_hash = ?")?;
        let path: Option<String> = stmt.query_row([hash], |row| row.get(0)).ok();
        Ok(path.map(PathBuf::from))
    }

    /// Remove a cache entry.
    pub fn remove(&self, hash: &str) -> Result<()> {
        // Get the file path without touching access stats
        if let Some(file_path) = self.get_path_only(hash)? {
            if file_path.exists() {
                fs::remove_file(&file_path)?;
            }
        }

        // Remove from database
        let conn = self.connect()?;
        conn.execute(
            "DELETE FROM cache_entries WHERE materialized_hash = ?",
            [hash],
        )?;

        Ok(())
    }

    /// Get total cache size in bytes.
    pub fn total_size(&self) -> Result<u64> {
        let conn = self.connect()?;

        let size: i64 = conn.query_row(
            "SELECT COALESCE(SUM(byte_size), 0) FROM cache_entries",
            [],
            |row| row.get(0),
        )?;

        Ok(size.try_into().unwrap_or(0))
    }

    /// Get number of cache entries.
    pub fn count(&self) -> Result<usize> {
        let conn = self.connect()?;

        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM cache_entries", [], |row| row.get(0))?;

        Ok(count as usize)
    }
}

/// Get the default cache directory.
///
/// Respects `OZZY_CACHE_DIR` env var if set, otherwise uses `~/.ozzy/cache/`.
pub fn default_cache_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("OZZY_CACHE_DIR") {
        return PathBuf::from(dir);
    }
    dirs::home_dir()
        .map(|h| h.join(".ozzy/cache"))
        .unwrap_or_else(|| PathBuf::from(".ozzy/cache"))
}

/// Format byte size for display.
pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_cache_operations() {
        let dir = tempdir().unwrap();
        let cache = LocalCache::open_at(dir.path()).unwrap();

        // Create a test parquet file
        let test_file = dir.path().join("test.parquet");
        fs::write(&test_file, b"test data").unwrap();

        // Store in cache
        let hash = "abc123";
        let path = cache
            .put(hash, "x86_64-linux", &test_file, Some(100))
            .unwrap();

        assert!(path.exists());
        assert!(cache.contains(hash).unwrap());

        // Retrieve
        let entry = cache.get(hash).unwrap().unwrap();
        assert_eq!(entry.materialized_hash, hash);
        assert_eq!(entry.row_count, Some(100));

        // Remove
        cache.remove(hash).unwrap();
        assert!(!cache.contains(hash).unwrap());
    }

    #[test]
    fn test_cache_list_and_size() {
        let dir = tempdir().unwrap();
        let cache = LocalCache::open_at(dir.path()).unwrap();

        // Create test files
        for i in 0..3 {
            let test_file = dir.path().join(format!("test{}.parquet", i));
            fs::write(&test_file, format!("test data {}", i)).unwrap();
            cache
                .put(&format!("hash{}", i), "x86_64-linux", &test_file, None)
                .unwrap();
        }

        assert_eq!(cache.count().unwrap(), 3);
        assert!(cache.total_size().unwrap() > 0);

        let entries = cache.list().unwrap();
        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(500), "500 B");
        assert_eq!(format_size(1500), "1.46 KB");
        assert_eq!(format_size(1_500_000), "1.43 MB");
        assert_eq!(format_size(1_500_000_000), "1.40 GB");
    }

    #[test]
    fn test_get_returns_none_for_deleted_file() {
        let dir = tempdir().unwrap();
        let cache = LocalCache::open_at(&dir.path().join("cache.db")).unwrap();

        // Create a test file and add to cache
        let test_file = dir.path().join("test.parquet");
        fs::write(&test_file, "test data").unwrap();
        cache
            .put("hash_deleted", "x86_64-linux", &test_file, None)
            .unwrap();

        // Verify it's in the cache
        assert!(cache.get("hash_deleted").unwrap().is_some());

        // Delete the file externally
        let entry = cache.get("hash_deleted").unwrap().unwrap();
        fs::remove_file(&entry.file_path).unwrap();

        // Now get should return None and clean up the DB entry
        assert!(
            cache.get("hash_deleted").unwrap().is_none(),
            "get() should return None when cached file is deleted"
        );
        // Verify DB entry was also cleaned up
        assert_eq!(cache.count().unwrap(), 0);
    }
}
