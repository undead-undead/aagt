//! OpenRouter provider implementation

use async_trait::async_trait;

use crate::{Error, Result, Message, StreamingResponse, ToolDefinition, Provider};
use crate::openai::OpenAI;

/// OpenRouter API client (OpenAI compatible with model routing)
pub struct OpenRouter {
    inner: OpenAI,
}

impl OpenRouter {
    /// Create from API key
    pub fn new(api_key: impl Into<String>) -> Result<Self> {
        let inner = OpenAI::with_base_url(api_key, "https://openrouter.ai/api/v1")?;
        Ok(Self { inner })
    }

    /// Create from environment variable
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("OPENROUTER_API_KEY")
            .map_err(|_| Error::ProviderAuth("OPENROUTER_API_KEY not set".to_string()))?;
        Self::new(api_key)
    }
}

#[async_trait]
impl Provider for OpenRouter {
    async fn stream_completion(
        &self,
        request: aagt_core::agent::provider::ChatRequest,
    ) -> Result<StreamingResponse> {
        self.inner.stream_completion(request).await
    }

    fn name(&self) -> &'static str {
        "openrouter"
    }
}

/// Popular models on OpenRouter
/// Claude 3.5 Sonnet via OpenRouter
pub const CLAUDE_3_5_SONNET: &str = "anthropic/claude-3.5-sonnet";
/// OpenAI GPT-4o via OpenRouter
pub const GPT_4O: &str = "openai/gpt-4o";
/// Gemini 2.0 Flash via OpenRouter
pub const GEMINI_FLASH: &str = "google/gemini-2.0-flash-exp";
/// Llama 3.3 70B via OpenRouter
pub const LLAMA_70B: &str = "meta-llama/llama-3.3-70b-instruct";
