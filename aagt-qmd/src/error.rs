use thiserror::Error;

#[derive(Error, Debug)]
pub enum QmdError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Document not found: {0}")]
    DocumentNotFound(String),

    #[error("Collection not found: {0}")]
    CollectionNotFound(String),

    #[error("Invalid virtual path: {0}")]
    InvalidVirtualPath(String),

    #[error("Invalid docid: {0}")]
    InvalidDocid(String),

    #[error("Glob pattern error: {0}")]
    GlobPattern(#[from] glob::PatternError),

    #[error("Content hash mismatch")]
    HashMismatch,

    #[error("{0}")]
    Custom(String),
}

pub type Result<T> = std::result::Result<T, QmdError>;
