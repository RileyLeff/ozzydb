#![recursion_limit = "4096"]

//! OzzyDB Registry Server Library
//!
//! The server owns the v4 registry, artifact, and execution control plane.
//! Auth, storage, and project ownership survive from earlier versions, but the
//! runtime model is now centered on typed artifacts, registry snapshots, and
//! published project revisions.

pub mod api;
pub mod auth;
pub mod compute;
pub mod config;
pub mod db;
pub mod environments;
pub mod git;
mod publication;
pub mod registry;
pub mod runners;
pub mod storage;
pub mod verification;

use std::sync::Arc;

pub use config::Config;
pub use db::Database;
pub use git::GitHubProvider;
pub use registry::{RegistrySnapshot, RegistrySnapshotCache};
pub use storage::ContentStorage;

/// Application state shared across handlers.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: Database,
    pub storage: ContentStorage,
    pub git: GitHubProvider,
    pub registry_snapshots: RegistrySnapshotCache,
    /// Compute provider registry (may be empty when compute is disabled).
    pub compute: compute::ComputeRegistry,
}
