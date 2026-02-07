//! Tiered cache implementation combining local (L1) and remote (L2) caches.
//!
//! Read-through behavior:
//! 1. Check L1 (local cache)
//! 2. On miss, check L2 (remote cache)
//! 3. On L2 hit, download and populate L1
//!
//! Write-through behavior:
//! 1. Always write to L1
//! 2. Optionally write to L2 based on policy (auto_push)

use std::path::{Path, PathBuf};

use super::backend::{CacheBackend, CacheEntry};
use super::config::TieredCacheConfig;
use super::local::LocalCache;
use super::remote::RemoteCache;
use crate::error::Result;
use crate::platform::PlatformFingerprint;

/// Tiered cache combining local (L1) and remote (L2) storage.
pub struct TieredCache {
    local: LocalCache,
    remote: Option<RemoteCache>,
    auto_pull: bool,
    auto_push: bool,
    fail_on_remote_error: bool,
    platform: String,
}

impl TieredCache {
    /// Create a new tiered cache from configuration.
    pub async fn new(config: &TieredCacheConfig) -> Result<Self> {
        let local = LocalCache::open()?;
        let platform = PlatformFingerprint::detect().short_string();

        let (remote, auto_pull, auto_push, fail_on_remote_error) = match &config.remote {
            Some(remote_config) if remote_config.is_configured() => {
                let remote = RemoteCache::new(remote_config).await?;
                (
                    Some(remote),
                    remote_config.policy.auto_pull,
                    remote_config.policy.auto_push,
                    remote_config.policy.fail_on_remote_error,
                )
            }
            _ => (None, false, false, false),
        };

        Ok(Self {
            local,
            remote,
            auto_pull,
            auto_push,
            fail_on_remote_error,
            platform,
        })
    }

    /// Create a tiered cache with only local storage (no remote).
    pub fn local_only() -> Result<Self> {
        let local = LocalCache::open()?;
        let platform = PlatformFingerprint::detect().short_string();

        Ok(Self {
            local,
            remote: None,
            auto_pull: false,
            auto_push: false,
            fail_on_remote_error: false,
            platform,
        })
    }

    /// Check if remote cache is configured.
    pub fn has_remote(&self) -> bool {
        self.remote.is_some()
    }

    /// Get the local cache path for a hash if it exists.
    pub fn get_local_path(&self, hash: &str) -> Result<Option<PathBuf>> {
        self.local.get_path(hash)
    }

    /// Check if an entry exists in L1 (local).
    pub fn contains_local(&self, hash: &str) -> Result<bool> {
        self.local.contains(hash)
    }

    /// Check if an entry exists in L2 (remote).
    pub async fn contains_remote(&self, hash: &str) -> Result<bool> {
        match &self.remote {
            Some(remote) => remote.contains(hash).await,
            None => Ok(false),
        }
    }

    /// Get entry with read-through behavior.
    ///
    /// 1. Check L1 (local)
    /// 2. If auto_pull enabled and L1 miss, check L2 (remote)
    /// 3. On L2 hit, download to L1 and return
    pub async fn get_path(&self, hash: &str) -> Result<Option<PathBuf>> {
        // Check L1 first
        if let Some(path) = self.local.get_path(hash)? {
            if path.exists() {
                return Ok(Some(path));
            }
        }

        // L1 miss - check L2 if auto_pull enabled
        if self.auto_pull {
            if let Some(remote) = &self.remote {
                match remote.contains(hash).await {
                    Ok(true) => {
                        let dest = self.local.cache_path_for(hash);
                        match remote.download(hash, &dest).await {
                            Ok(true) => {
                                self.local.register_downloaded(hash, &self.platform)?;
                                return Ok(Some(dest));
                            }
                            Ok(false) => {} // Download returned false, treat as miss
                            Err(e) if self.fail_on_remote_error => return Err(e),
                            Err(e) => {
                                eprintln!("Warning: Remote cache download failed: {}", e);
                            }
                        }
                    }
                    Ok(false) => {} // Not in remote
                    Err(e) if self.fail_on_remote_error => return Err(e),
                    Err(e) => {
                        eprintln!("Warning: Remote cache check failed: {}", e);
                    }
                }
            }
        }

        Ok(None)
    }

    /// Store entry in cache with write-through behavior.
    ///
    /// 1. Always writes to L1 (local)
    /// 2. If auto_push enabled, also writes to L2 (remote)
    pub async fn put(
        &self,
        hash: &str,
        platform: &str,
        source_path: &Path,
        row_count: Option<u64>,
    ) -> Result<PathBuf> {
        // Always write to L1
        let cached_path = self.local.put(hash, platform, source_path, row_count)?;

        // Write to L2 if auto_push enabled
        if self.auto_push {
            if let Some(remote) = &self.remote {
                match remote.upload(hash, platform, source_path, row_count).await {
                    Ok(()) => {}
                    Err(e) if self.fail_on_remote_error => return Err(e),
                    Err(e) => {
                        eprintln!("Warning: Remote cache push failed: {}", e);
                    }
                }
            }
        }

        Ok(cached_path)
    }

    /// Explicitly push an entry from L1 to L2.
    pub async fn push(&self, hash: &str) -> Result<bool> {
        let remote = match &self.remote {
            Some(r) => r,
            None => return Ok(false),
        };

        // Get local entry
        let local_path = match self.local.get_path(hash)? {
            Some(p) if p.exists() => p,
            _ => return Ok(false),
        };

        // Get entry metadata
        let entry = self.local.get(hash)?;
        let row_count = entry.as_ref().and_then(|e| e.row_count);

        // Upload to remote
        remote
            .upload(hash, &self.platform, &local_path, row_count)
            .await?;

        Ok(true)
    }

    /// Explicitly pull an entry from L2 to L1.
    pub async fn pull(&self, hash: &str) -> Result<bool> {
        let remote = match &self.remote {
            Some(r) => r,
            None => return Ok(false),
        };

        // Check if already in L1
        if self.local.contains(hash)? {
            if let Some(path) = self.local.get_path(hash)? {
                if path.exists() {
                    return Ok(true); // Already have it
                }
            }
        }

        // Check if exists in L2
        if !remote.contains(hash).await? {
            return Ok(false);
        }

        // Download from L2 to L1
        let dest = self.local.cache_path_for(hash);
        if remote.download(hash, &dest).await? {
            self.local.register_downloaded(hash, &self.platform)?;
            return Ok(true);
        }

        Ok(false)
    }

    /// Push all local entries to remote.
    pub async fn push_all(&self) -> Result<PushPullStats> {
        let remote = match &self.remote {
            Some(r) => r,
            None => {
                return Ok(PushPullStats {
                    pushed: 0,
                    pulled: 0,
                    skipped: 0,
                    errors: 0,
                });
            }
        };

        let local_entries = self.local.list()?;
        let mut stats = PushPullStats::default();

        for entry in local_entries {
            // Check if already in remote
            if remote.contains(&entry.materialized_hash).await? {
                stats.skipped += 1;
                continue;
            }

            // Get local path
            if let Some(local_path) = self.local.get_path(&entry.materialized_hash)? {
                if local_path.exists() {
                    match remote
                        .upload(
                            &entry.materialized_hash,
                            &entry.platform,
                            &local_path,
                            entry.row_count,
                        )
                        .await
                    {
                        Ok(()) => stats.pushed += 1,
                        Err(_) => stats.errors += 1,
                    }
                } else {
                    stats.errors += 1;
                }
            }
        }

        Ok(stats)
    }

    /// Pull all remote entries (for current platform) to local.
    pub async fn pull_all(&self) -> Result<PushPullStats> {
        let remote = match &self.remote {
            Some(r) => r,
            None => {
                return Ok(PushPullStats {
                    pushed: 0,
                    pulled: 0,
                    skipped: 0,
                    errors: 0,
                });
            }
        };

        let remote_entries = remote.list().await?;
        let mut stats = PushPullStats::default();

        for entry in remote_entries {
            // Check if already in local
            if self.local.contains(&entry.materialized_hash)? {
                if let Some(path) = self.local.get_path(&entry.materialized_hash)? {
                    if path.exists() {
                        stats.skipped += 1;
                        continue;
                    }
                }
            }

            // Download from remote
            let dest = self.local.cache_path_for(&entry.materialized_hash);
            match remote.download(&entry.materialized_hash, &dest).await {
                Ok(true) => {
                    if self
                        .local
                        .register_downloaded(&entry.materialized_hash, &entry.platform)
                        .is_ok()
                    {
                        stats.pulled += 1;
                    } else {
                        stats.errors += 1;
                    }
                }
                Ok(false) => stats.errors += 1,
                Err(_) => stats.errors += 1,
            }
        }

        Ok(stats)
    }

    /// Get cache status showing local and remote counts.
    pub async fn status(&self) -> Result<CacheStatus> {
        let local_entries = self.local.list()?;
        let local_count = local_entries.len();
        let local_size = self.local.total_size()?;

        let (remote_count, remote_platforms) = if let Some(remote) = &self.remote {
            let entries = remote.list().await?;
            let platforms = remote.list_platforms().await?;
            (entries.len(), platforms)
        } else {
            (0, vec![])
        };

        // Count how many local entries are also in remote
        let mut synced_count = 0;
        if let Some(remote) = &self.remote {
            for entry in &local_entries {
                if remote.contains(&entry.materialized_hash).await? {
                    synced_count += 1;
                }
            }
        }

        Ok(CacheStatus {
            local_count,
            local_size,
            remote_count,
            remote_platforms,
            synced_count,
            remote_configured: self.remote.is_some(),
        })
    }

    /// Get entries that exist locally but not remotely.
    pub async fn pending_push(&self) -> Result<Vec<CacheEntry>> {
        let remote = match &self.remote {
            Some(r) => r,
            None => return Ok(vec![]),
        };

        let local_entries = self.local.list()?;
        let mut pending = Vec::new();

        for entry in local_entries {
            if !remote.contains(&entry.materialized_hash).await? {
                pending.push(entry);
            }
        }

        Ok(pending)
    }

    /// Get entries that exist remotely but not locally.
    pub async fn pending_pull(&self) -> Result<Vec<CacheEntry>> {
        let remote = match &self.remote {
            Some(r) => r,
            None => return Ok(vec![]),
        };

        let remote_entries = remote.list().await?;
        let mut pending = Vec::new();

        for entry in remote_entries {
            let in_local = if self.local.contains(&entry.materialized_hash)? {
                if let Some(path) = self.local.get_path(&entry.materialized_hash)? {
                    path.exists()
                } else {
                    false
                }
            } else {
                false
            };

            if !in_local {
                pending.push(entry);
            }
        }

        Ok(pending)
    }

    /// Get access to the underlying local cache.
    pub fn local(&self) -> &LocalCache {
        &self.local
    }

    /// Get access to the underlying remote cache (if configured).
    pub fn remote(&self) -> Option<&RemoteCache> {
        self.remote.as_ref()
    }
}

/// Statistics from push/pull operations.
#[derive(Debug, Default, Clone)]
pub struct PushPullStats {
    pub pushed: usize,
    pub pulled: usize,
    pub skipped: usize,
    pub errors: usize,
}

/// Cache status information.
#[derive(Debug, Clone)]
pub struct CacheStatus {
    pub local_count: usize,
    pub local_size: u64,
    pub remote_count: usize,
    pub remote_platforms: Vec<String>,
    pub synced_count: usize,
    pub remote_configured: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_local_only_cache() {
        let cache = TieredCache::local_only().unwrap();
        assert!(!cache.has_remote());
    }
}
