//! # AAGT Core - AI Agent for Trading
//!
//! Core types, traits, and abstractions for the AAGT framework.
//!
//! This crate provides:
//! - Agent system (`agent`) - AI agent with tool calling
//! - Tool definitions (`tool`) - Define callable tools
//! - Message types (`message`) - Conversation messages
//! - Streaming (`streaming`) - Stream response handling
//! - Memory (`memory`) - Short and long-term memory
//! - Strategy (`strategy`) - Automated trading pipelines
//! - Risk control (`risk`) - Trading safeguards
//! - Simulation (`simulation`) - Trade simulation
//! - Multi-agent (`multi_agent`) - Agent coordination

#![warn(missing_docs)]

pub mod agent;
pub mod capabilities;
pub mod context;
pub mod error;
pub mod logging;
pub mod maintenance;
pub mod memory;
pub mod message;
pub mod multi_agent;
pub mod notification;
pub mod pipeline;
pub mod provider;
pub mod rag;
pub mod risk;
pub mod simulation;
pub mod skill;
pub mod store;
pub mod strategy;
pub mod streaming;
pub mod tool;

pub use anyhow;

/// Prelude - commonly used types
pub mod prelude {
    pub use crate::agent::{Agent, AgentBuilder, AgentConfig};
    pub use crate::context::{ContextConfig, ContextManager};
    pub use crate::error::{Error, Result};
    pub use crate::maintenance::{MaintenanceConfig, MaintenanceManager};
    pub use crate::memory::{LongTermMemory, Memory, MemoryManager, QmdMemory, ShortTermMemory};
    pub use crate::message::{Content, Message, Role, ToolCall};
    pub use crate::multi_agent::{AgentRole, Coordinator, MultiAgent};
    pub use crate::notification::{Notifier, NotifyChannel};
    pub use crate::provider::{CircuitBreakerConfig, Provider, ResilientProvider};
    pub use crate::risk::{
        RiskCheck, RiskCheckBuilder, RiskCheckResult, RiskConfig, RiskManager, TradeContext,
    };
    pub use crate::simulation::{SimulationRequest, SimulationResult, Simulator};
    pub use crate::skill::{DynamicSkill, SkillExecutionConfig, SkillLoader, SkillMetadata};
    pub use crate::strategy::{Action, Condition, Pipeline, Strategy};
    pub use crate::streaming::{StreamingChoice, StreamingResponse};
    pub use crate::tool::{Tool, ToolDefinition, ToolSet};
}
