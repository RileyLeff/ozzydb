//! Git provider integration for fetching repository content.
//!
//! GitHub is the first (and currently only) implementation.
//! The provider abstraction exists in the architecture for future
//! GitLab/Gitea support, but we use the concrete type for now.

pub mod github;

pub use github::GitHubProvider;
