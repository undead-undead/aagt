pub mod cache;
pub mod context;
pub mod core;
pub mod history;
pub mod memory;
pub mod message;
pub mod multi_agent;
pub mod namespaced_memory; // NEW: Namespaced shared memory
pub mod personality;
pub mod provider;
pub mod scheduler;
pub mod session;
pub mod streaming;
pub mod swarm;

pub use core::{Agent, AgentBuilder, AgentConfig};
pub use namespaced_memory::{MemoryEntry, NamespacedMemory};
pub use session::{AgentSession, SessionStatus};
// NEW
