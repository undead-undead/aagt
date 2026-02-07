//! # AAGT Core - AI Agent for Trading
//!
//! Core types, traits, and abstractions for the AAGT framework.
//!
//! This crate provides:
//! - Agent system (`agent`)
//! - Trading logic (`trading`)
//! - Skills & Tools (`skills`)
//! - Knowledge & Memory (`knowledge`)
//! - Infrastructure (`infra`)

pub mod error;

pub mod agent;
pub mod bus; // NEW: Message Bus
pub mod infra;
pub mod knowledge;
pub mod prelude;
pub mod skills;
#[cfg(feature = "trading")]
pub mod trading;

// Re-export common types for convenience
pub use agent::core::{Agent, AgentBuilder, AgentConfig};
pub use agent::message::{Content, Message, Role};
pub use error::{Error, Result};
