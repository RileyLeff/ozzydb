//! Compute backend — pluggable execution engine for transform containers.
//!
//! The `ComputeBackend` trait allows swapping Docker (local) for Fly Machines
//! (cloud) or other providers.

pub mod docker;
pub mod types;

pub use types::{ComputeRequest, ComputeResult, InputSpec};
