//! OzzyDB Core - Data management platform for scientific computing
//!
//! v2: OzzyDB is a switchboard. Git owns code, container registries own environments,
//! OzzyDB owns data, wires everything together, orchestrates compute, and caches results.

pub mod canon;
pub mod error;
pub mod hash;
pub mod platform;
pub mod schema;
pub mod toml_spec;

pub use error::{Error, Result};
