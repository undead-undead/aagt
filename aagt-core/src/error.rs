//! Error types for the aagt framework

use thiserror::Error;

/// Result type alias using aagt's Error
pub type Result<T> = std::result::Result<T, Error>;

/// Main error type for the aagt framework
#[derive(Debug, Error)]
pub enum Error {
    // ============ Agent Errors ============
    /// Agent is not properly configured
    #[error("Agent configuration error: {0}")]
    AgentConfig(String),

    /// Agent execution failed
    #[error("Agent execution error: {0}")]
    AgentExecution(String),

    // ============ Provider Errors ============
    /// Provider API error
    #[error("Provider API error: {0}")]
    ProviderApi(String),

    /// Provider authentication failed
    #[error("Provider authentication error: {0}")]
    ProviderAuth(String),

    /// Provider rate limit exceeded
    #[error("Provider rate limit exceeded: retry after {retry_after_secs}s")]
    ProviderRateLimit {
        /// Seconds to wait before retrying
        retry_after_secs: u64,
    },

    // ============ Tool Errors ============
    /// Tool not found in agent's toolset
    #[error("Tool not found: {0}")]
    ToolNotFound(String),

    /// Tool execution failed
    #[error("Tool execution error: {tool_name} - {message}")]
    ToolExecution {
        /// Name of the tool that failed
        tool_name: String,
        /// Error message
        message: String,
    },

    /// Invalid tool arguments
    #[error("Invalid tool arguments for {tool_name}: {message}")]
    ToolArguments {
        /// Name of the tool
        tool_name: String,
        /// Error message
        message: String,
    },

    // ============ Message Errors ============
    /// Message parsing failed
    #[error("Message parse error: {0}")]
    MessageParse(String),

    /// Message serialization failed
    #[error("Message serialization error: {0}")]
    MessageSerialize(#[from] serde_json::Error),

    // ============ Streaming Errors ============
    /// Stream interrupted
    #[error("Stream interrupted: {0}")]
    StreamInterrupted(String),

    /// Stream timeout
    #[error("Stream timeout after {timeout_secs}s")]
    StreamTimeout {
        /// Timeout duration in seconds
        timeout_secs: u64,
    },

    // ============ Memory Errors ============
    /// Memory storage error
    #[error("Memory storage error: {0}")]
    MemoryStorage(String),

    /// Memory retrieval error
    #[error("Memory retrieval error: {0}")]
    MemoryRetrieval(String),

    // ============ Strategy Errors ============
    /// Strategy configuration error
    #[error("Strategy configuration error: {0}")]
    StrategyConfig(String),

    /// Strategy execution error
    #[error("Strategy execution error: {0}")]
    StrategyExecution(String),

    /// Condition evaluation error
    #[error("Condition evaluation error: {0}")]
    ConditionEvaluation(String),

    // ============ Risk Control Errors ============
    /// Risk check failed - transaction blocked
    #[error("Risk check failed: {check_name} - {reason}")]
    RiskCheckFailed {
        /// Name of the risk check
        check_name: String,
        /// Reason for failure
        reason: String,
    },

    /// Risk limit exceeded
    #[error("Risk limit exceeded: {limit_type} - current: {current}, max: {max}")]
    RiskLimitExceeded {
        /// Type of limit
        limit_type: String,
        /// Current value
        current: String,
        /// Maximum allowed value
        max: String,
    },

    // ============ Simulation Errors ============
    /// Simulation failed
    #[error("Simulation error: {0}")]
    Simulation(String),

    // ============ Multi-Agent Errors ============
    /// Agent coordination error
    #[error("Agent coordination error: {0}")]
    AgentCoordination(String),

    /// Agent communication error
    #[error("Agent communication error: {0}")]
    AgentCommunication(String),

    // ============ Network Errors ============
    /// HTTP request failed
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    // ============ System Errors ============
    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    // ============ Generic Errors ============
    /// Internal error
    #[error("Internal error: {0}")]
    Internal(String),

    /// Any other error
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl Error {
    /// Create a new agent configuration error
    pub fn agent_config(msg: impl Into<String>) -> Self {
        Self::AgentConfig(msg.into())
    }

    /// Create a new tool execution error
    pub fn tool_execution(tool_name: impl Into<String>, message: impl Into<String>) -> Self {
        Self::ToolExecution {
            tool_name: tool_name.into(),
            message: message.into(),
        }
    }

    /// Create a new risk check failed error
    pub fn risk_check_failed(check_name: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::RiskCheckFailed {
            check_name: check_name.into(),
            reason: reason.into(),
        }
    }

    /// Check if this error is retryable
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::ProviderRateLimit { .. }
                | Self::StreamInterrupted(_)
                | Self::StreamTimeout { .. }
                | Self::Http(_)
        )
    }
}
