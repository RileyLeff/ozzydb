//! OzzyDB Registry Server Library
//!
//! This module exposes the server's components for use by the binary and tests.

pub mod api;
pub mod auth;
pub mod compute;
pub mod config;
pub mod db;
pub mod storage;

use std::sync::Arc;

pub use config::Config;
pub use db::Database;
pub use storage::ContentStorage;

/// Application state shared across handlers.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: Database,
    pub storage: ContentStorage,
    /// Storage for materialized transform outputs (separate R2 prefix).
    pub materialized_storage: ContentStorage,
}
