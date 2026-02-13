//! OzzyDB Registry Server Library
//!
//! v2: The server stores data atoms, manages environments, orchestrates compute,
//! and caches materialized results. Auth, storage, and project ownership survive
//! from v1; compute and the wire protocol are being rebuilt.

pub mod api;
pub mod auth;
pub mod compute;
pub mod config;
pub mod db;
pub mod environments;
pub mod git;
pub mod runners;
pub mod storage;

use std::sync::Arc;

pub use config::Config;
pub use db::Database;
pub use git::GitHubProvider;
pub use storage::ContentStorage;

/// Application state shared across handlers.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: Database,
    pub storage: ContentStorage,
    pub git: GitHubProvider,
}
