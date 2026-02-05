//! OzzyDB Core - Version control for data transformations
//!
//! OzzyDB versions data transformations rather than data itself. Raw data is immutable.
//! Derived data is always reproducible from `(raw, transforms, params, dependencies, platform)`.

pub mod cache;
pub mod canon;
pub mod commit;
pub mod error;
pub mod hash;
pub mod platform;
pub mod project;
pub mod runtime;
pub mod schema;

pub use error::{Error, Result};
pub use project::Project;
