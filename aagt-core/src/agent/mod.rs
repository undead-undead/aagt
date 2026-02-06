pub mod context;
pub mod core;
pub mod memory;
pub mod message;
pub mod multi_agent;
pub mod namespaced_memory; // NEW: Namespaced shared memory
pub mod personality;
pub mod provider;
pub mod scheduler;
pub mod streaming;

pub use core::{Agent, AgentBuilder, AgentConfig};
pub use namespaced_memory::{MemoryEntry, NamespacedMemory}; // NEW
