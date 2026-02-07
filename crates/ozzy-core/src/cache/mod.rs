//! Cache module for computed transform outputs.
//!
//! Provides local caching for transform results.
//! Cache entries are content-addressed by their materialized hash.

mod local;

// Re-export public types
pub use local::{LocalCache, default_cache_dir, format_size};
