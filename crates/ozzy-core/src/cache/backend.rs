//! Cache backend trait and common types.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Cache entry metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    pub materialized_hash: String,
    pub platform: String,
    pub row_count: Option<u64>,
    pub byte_size: Option<u64>,
    pub created_at: DateTime<Utc>,
    pub last_accessed: DateTime<Utc>,
    pub access_count: u64,
    /// Location of the cached data
    pub location: CacheLocation,
}

impl CacheEntry {
    /// Get the file path if this is a local cache entry.
    pub fn file_path(&self) -> Option<&PathBuf> {
        match &self.location {
            CacheLocation::Local(path) => Some(path),
            CacheLocation::Remote { .. } => None,
        }
    }
}

/// Location of a cache entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CacheLocation {
    /// Local file path
    Local(PathBuf),
    /// Remote object storage
    Remote {
        bucket: String,
        key: String,
    },
}

/// Filter for cache queries.
#[derive(Debug, Clone, Default)]
pub struct CacheFilter {
    /// Filter by platform (e.g., "aarch64-macos")
    pub platform: Option<String>,
    /// Minimum file size in bytes
    pub min_size: Option<u64>,
    /// Maximum file size in bytes
    pub max_size: Option<u64>,
    /// Only entries accessed since this time
    pub since: Option<DateTime<Utc>>,
}

/// Trait for cache backend implementations.
///
/// Both LocalCache and RemoteCache implement this trait, allowing for
/// flexible composition in TieredCache.
pub trait CacheBackend: Send + Sync {
    /// Get cache entry metadata by hash.
    fn get(&self, hash: &str) -> crate::error::Result<Option<CacheEntry>>;

    /// Check if a hash exists in the cache.
    fn contains(&self, hash: &str) -> crate::error::Result<bool>;

    /// Store a file in the cache.
    fn put(
        &self,
        hash: &str,
        platform: &str,
        source_path: &std::path::Path,
        row_count: Option<u64>,
    ) -> crate::error::Result<PathBuf>;

    /// Remove an entry from the cache.
    fn remove(&self, hash: &str) -> crate::error::Result<()>;

    /// Get total cache size in bytes.
    fn total_size(&self) -> crate::error::Result<u64>;

    /// Get number of cache entries.
    fn count(&self) -> crate::error::Result<usize>;
}
