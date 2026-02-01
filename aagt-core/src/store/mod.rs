//! Storage implementations for AAGT
//!
//! Includes lightweight implementations like FileStore (JSONL).

pub mod file;
pub use file::{FileStore, FileStoreConfig};
