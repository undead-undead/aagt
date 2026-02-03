//! Logging configuration with rotation support
//!
//! # Example
//!
//! ```rust
//! use aagt_core::logging::init_logging;
//!
//! init_logging("logs", "agent.log", "info").unwrap();
//! ```

use crate::error::Result;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// Initialize logging with file rotation
///
/// - `directory`: Directory to store logs
/// - `filename_prefix`: Prefix for log files (e.g. "aagt.log")
/// - `level`: Default log level (e.g. "info", "debug")
pub fn init_logging(directory: &str, filename_prefix: &str, level: &str) -> Result<()> {
    // 1. File Appender with Rotation (Daily)
    let file_appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(filename_prefix)
        .build(directory)
        .map_err(|e| {
            crate::error::Error::Internal(format!("Failed to create log appender: {}", e))
        })?;

    // 2. Formatting layers
    // Console: Human readable
    let console_layer = fmt::layer()
        .with_target(false) // meaningful targets are better, but for clean output maybe false
        .compact();

    // File: JSON for parsing or full text
    // For lightweight usage, text is often easier to read than JSON. Let's stick to text for now.
    let file_layer = fmt::layer().with_writer(file_appender).with_ansi(false);

    // 3. Filter
    // Allow RUST_LOG env var to override, otherwise use default
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));

    // 4. Registry
    tracing_subscriber::registry()
        .with(filter)
        .with(console_layer)
        .with(file_layer)
        .try_init()
        .map_err(|e| crate::error::Error::Internal(format!("Failed to init tracing: {}", e)))?;

    Ok(())
}
