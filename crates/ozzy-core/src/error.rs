use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Not in an OzzyDB project directory (ozzy.toml not found)")]
    NotInProject,

    #[error("Project already initialized")]
    ProjectAlreadyExists,

    #[error("Data source '{0}' not found")]
    DataSourceNotFound(String),

    #[error("Data source '{0}' already exists")]
    DataSourceAlreadyExists(String),

    #[error("Transform '{0}' not found")]
    TransformNotFound(String),

    #[error("Transform '{0}' already exists")]
    TransformAlreadyExists(String),

    #[error("Endpoint '{0}' not found")]
    EndpointNotFound(String),

    #[error("Endpoint '{0}' already exists")]
    EndpointAlreadyExists(String),

    #[error("Commit '{0}' not found")]
    CommitNotFound(String),

    #[error("Invalid transform file format: {0}")]
    InvalidTransformFormat(String),

    #[error("Schema mismatch: {0}")]
    SchemaMismatch(String),

    #[error("File not found: {0}")]
    FileNotFound(String),

    #[error("Invalid parquet file: {0}")]
    InvalidParquet(String),

    #[error("Cache error: {0}")]
    CacheError(String),

    #[error("Runtime error: {0}")]
    RuntimeError(String),

    #[error("Python execution failed: {0}")]
    PythonError(String),

    #[error("No changes to commit")]
    NothingToCommit,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("TOML serialization error: {0}")]
    TomlSer(#[from] toml::ser::Error),

    #[error("TOML deserialization error: {0}")]
    TomlDe(#[from] toml::de::Error),

    #[error("Arrow error: {0}")]
    Arrow(#[from] arrow::error::ArrowError),

    #[error("Parquet error: {0}")]
    Parquet(#[from] parquet::errors::ParquetError),

    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
