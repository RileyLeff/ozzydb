//! Registry protocol types and client for remote project sharing.
//!
//! This module provides shared types for communication between:
//! - The ozzy-cli (push, pull, fetch commands)
//! - The ozzy-server (registry API)
//! - The Python client (remote fetch support)

pub mod client;
pub mod protocol;

pub use client::RegistryClient;
pub use protocol::*;
