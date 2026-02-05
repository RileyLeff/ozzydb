//! Cache module for computed transform outputs.
//!
//! Provides both local caching (L1) and remote caching (L2) for transform results.
//! Cache entries are content-addressed by their materialized hash.

mod backend;
mod config;
mod local;
mod remote;
mod tiered;

// Re-export public types
pub use backend::{CacheBackend, CacheEntry, CacheFilter, CacheLocation};
pub use config::{RemoteCacheConfig, RemoteCachePolicy, TieredCacheConfig};
pub use local::{default_cache_dir, format_size, LocalCache};
pub use remote::{CacheEntryMeta, RemoteCache};
pub use tiered::{CacheStatus, PushPullStats, TieredCache};
