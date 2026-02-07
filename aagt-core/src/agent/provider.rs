//! Provider trait for LLM integrations

use async_trait::async_trait;

use crate::error::Result;
use crate::agent::message::Message;
use crate::agent::streaming::StreamingResponse;
use crate::skills::tool::ToolDefinition;

mod resilient;

pub use resilient::{ResilientProvider, CircuitBreakerConfig};

/// Request for a chat completion
#[derive(Debug, Clone, Default)]
pub struct ChatRequest {
    /// Model name to use
    pub model: String,
    /// Optional system prompt
    pub system_prompt: Option<String>,
    /// Conversation history
    pub messages: Vec<Message>,
    /// Available tools
    pub tools: Vec<ToolDefinition>,
    /// Optional temperature setting
    pub temperature: Option<f64>,
    /// Optional max tokens
    pub max_tokens: Option<u64>,
    /// Optional provider-specific parameters
    pub extra_params: Option<serde_json::Value>,
}

/// Trait for LLM providers
///
/// Implement this trait to add support for a new LLM provider.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Stream a completion request
    async fn stream_completion(
        &self,
        request: ChatRequest,
    ) -> Result<StreamingResponse>;

    /// Get provider name (for logging/debugging)
    fn name(&self) -> &'static str;

    /// Check if provider supports streaming
    fn supports_streaming(&self) -> bool {
        true
    }

    /// Check if provider supports tool calls
    fn supports_tools(&self) -> bool {
        true
    }
}
