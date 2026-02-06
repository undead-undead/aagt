//! Prelude: Re-exports common types for convenience
//!
//! # Usage
//! ```
//! use aagt_core::prelude::*;
//! ```

pub use crate::error::{Error, Result};

// Agent
pub use crate::agent::context::{ContextConfig, ContextInjector, ContextManager};
pub use crate::agent::core::{Agent, AgentBuilder, AgentConfig};
#[cfg(feature = "qmd")]
pub use crate::agent::memory::QmdMemory;
pub use crate::agent::memory::{
    LongTermMemory, Memory, MemoryEntry, MemoryManager, ShortTermMemory,
};
pub use crate::agent::message::{Content, ContentPart, ImageSource, Message, Role, ToolCall};
pub use crate::agent::personality::{Persona, Traits};
pub use crate::agent::provider::Provider;
pub use crate::agent::streaming::{StreamingChoice, StreamingResponse};

// Skills
pub use crate::skills::tool::{Tool, ToolDefinition};
pub use crate::skills::{DynamicSkill, SkillExecutionConfig, SkillLoader};

// Trading
pub use crate::trading::pipeline::{Context as PipelineContext, Pipeline, Step};
pub use crate::trading::risk::{
    RiskCheck, RiskCheckBuilder, RiskConfig, RiskManager, TradeContext,
};
pub use crate::trading::strategy::{Action, Condition, FileStrategyStore, Strategy, StrategyStore};

// Infra
pub use crate::infra::maintenance::{MaintenanceConfig, MaintenanceManager};
pub use crate::infra::notification::NotifyChannel;
