//! End-to-end tests for the OzzyDB registry server.
//!
//! v2: These tests will be rewritten as v2 endpoints are implemented.
//! The v1 compute pipeline, push/pull protocol, and fetch endpoint have been removed.
//! New tests will cover:
//! - Data atom upload → collection → endpoint → fetch (full pipeline)
//! - Environment image management
//! - Fly Machines compute execution
//! - Materialized cache behavior
//! - Collaborator and public project access control
//!
//! Requirements: Docker must be running.
//!
//! Run: cargo test -p ozzy-server --test e2e_tests -- --ignored
