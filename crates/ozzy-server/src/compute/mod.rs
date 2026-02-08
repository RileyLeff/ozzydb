//! Server-side transform execution.
//!
//! Runs transform pipelines in sandboxed Docker containers (gVisor by default).
//! Each transform gets `--network=none`, `--read-only`, resource limits, and
//! deterministic env vars. Results are cached in R2 by materialized hash.

pub mod executor;
pub mod image;
pub mod pipeline;
