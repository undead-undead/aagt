//! Provider trait for LLM integrations

use async_trait::async_trait;

use crate::error::Result;
use crate::message::Message;
use crate::streaming::StreamingResponse;
use crate::tool::ToolDefinition;

/// Trait for LLM providers
///
/// Implement this trait to add support for a new LLM provider.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Stream a completion request
    ///
    /// # Arguments
    /// * `model` - Model name to use
    /// * `system_prompt` - Optional system prompt
    /// * `messages` - Conversation history
    /// * `tools` - Available tools (empty if none)
    /// * `temperature` - Optional temperature setting
    /// * `max_tokens` - Optional max tokens
    /// * `extra_params` - Optional provider-specific parameters
    async fn stream_completion(
        &self,
        model: &str,
        system_prompt: Option<&str>,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
        temperature: Option<f64>,
        max_tokens: Option<u64>,
        extra_params: Option<serde_json::Value>,
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
