//! DeepSeek provider implementation

use async_trait::async_trait;

use crate::{Error, Result, Message, StreamingResponse, ToolDefinition, Provider, HttpConfig};
use crate::openai::OpenAI;

/// DeepSeek API client (OpenAI compatible)
pub struct DeepSeek {
    inner: OpenAI,
}

impl DeepSeek {
    /// Create from API key
    pub fn new(api_key: impl Into<String>) -> Result<Self> {
        let inner = OpenAI::with_base_url(api_key, "https://api.deepseek.com/v1")?;
        Ok(Self { inner })
    }

    /// Create from environment variable
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("DEEPSEEK_API_KEY")
            .map_err(|_| Error::ProviderAuth("DEEPSEEK_API_KEY not set".to_string()))?;
        Self::new(api_key)
    }
}

#[async_trait]
impl Provider for DeepSeek {
    async fn stream_completion(
        &self,
        model: &str,
        system_prompt: Option<&str>,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
        temperature: Option<f64>,
        max_tokens: Option<u64>,
        extra_params: Option<serde_json::Value>,
    ) -> Result<StreamingResponse> {
        self.inner
            .stream_completion(model, system_prompt, messages, tools, temperature, max_tokens, extra_params)
            .await
    }

    fn name(&self) -> &'static str {
        "deepseek"
    }
}

/// Common model constants
pub const DEEPSEEK_CHAT: &str = "deepseek-chat";
pub const DEEPSEEK_CODER: &str = "deepseek-coder";
