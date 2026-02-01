//! Agent system - the core AI agent abstraction

use std::sync::Arc;

use crate::error::{Error, Result};
use crate::message::Message;
use crate::provider::Provider;
use crate::streaming::StreamingResponse;
use crate::tool::{Tool, ToolSet};

/// Configuration for an agent
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// System prompt / preamble
    pub system_prompt: Option<String>,
    /// Model name to use
    pub model: String,
    /// Temperature for generation
    pub temperature: Option<f64>,
    /// Maximum tokens to generate
    pub max_tokens: Option<u64>,
    /// Additional provider-specific parameters
    pub extra_params: Option<serde_json::Value>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            system_prompt: None,
            model: "gpt-4o".to_string(),
            temperature: None,
            max_tokens: Some(4096),
            extra_params: None,
        }
    }
}

/// The main Agent struct - combines a provider with tools and configuration
pub struct Agent<P: Provider> {
    /// The LLM provider
    provider: Arc<P>,
    /// Tools available to this agent
    tools: ToolSet,
    /// Agent configuration
    config: AgentConfig,
}

impl<P: Provider> Agent<P> {
    /// Create a new agent builder
    pub fn builder(provider: P) -> AgentBuilder<P> {
        AgentBuilder::new(provider)
    }

    /// Send a prompt and get a response (non-streaming)
    pub async fn prompt(&self, prompt: impl Into<String>) -> Result<String> {
        let messages = vec![Message::user(prompt.into())];
        self.chat(messages).await
    }

    /// Send messages and get a response (non-streaming)
    pub async fn chat(&self, messages: Vec<Message>) -> Result<String> {
        let stream = self.stream_chat(messages).await?;
        stream.collect_text().await
    }

    /// Stream a prompt response
    pub async fn stream(&self, prompt: impl Into<String>) -> Result<StreamingResponse> {
        let messages = vec![Message::user(prompt.into())];
        self.stream_chat(messages).await
    }

    /// Stream a chat response
    pub async fn stream_chat(&self, messages: Vec<Message>) -> Result<StreamingResponse> {
        self.provider
            .stream_completion(
                &self.config.model,
                self.config.system_prompt.as_deref(),
                messages,
                self.tools.definitions().await,
                self.config.temperature,
                self.config.max_tokens,
                self.config.extra_params.clone(),
            )
            .await
    }

    /// Call a tool by name
    pub async fn call_tool(&self, name: &str, arguments: &str) -> Result<String> {
        self.tools.call(name, arguments).await
    }

    /// Check if agent has a tool
    pub fn has_tool(&self, name: &str) -> bool {
        self.tools.contains(name)
    }

    /// Get tool definitions for the LLM
    pub async fn tool_definitions(&self) -> Vec<crate::tool::ToolDefinition> {
        self.tools.definitions().await
    }

    /// Get the agent's configuration
    pub fn config(&self) -> &AgentConfig {
        &self.config
    }

    /// Get the model name
    pub fn model(&self) -> &str {
        &self.config.model
    }
}

/// Builder for creating agents
pub struct AgentBuilder<P: Provider> {
    provider: P,
    tools: ToolSet,
    config: AgentConfig,
}

impl<P: Provider> AgentBuilder<P> {
    /// Create a new builder with a provider
    pub fn new(provider: P) -> Self {
        Self {
            provider,
            tools: ToolSet::new(),
            config: AgentConfig::default(),
        }
    }

    /// Set the model to use
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.config.model = model.into();
        self
    }

    /// Set the system prompt
    pub fn system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.config.system_prompt = Some(prompt.into());
        self
    }

    /// Alias for system_prompt
    pub fn preamble(self, prompt: impl Into<String>) -> Self {
        self.system_prompt(prompt)
    }

    /// Set the temperature
    pub fn temperature(mut self, temp: f64) -> Self {
        self.config.temperature = Some(temp);
        self
    }

    /// Set max tokens
    pub fn max_tokens(mut self, tokens: u64) -> Self {
        self.config.max_tokens = Some(tokens);
        self
    }

    /// Add extra provider-specific parameters
    pub fn extra_params(mut self, params: serde_json::Value) -> Self {
        self.config.extra_params = Some(params);
        self
    }

    /// Add a tool
    pub fn tool<T: Tool + 'static>(mut self, tool: T) -> Self {
        self.tools.add(tool);
        self
    }

    /// Add a shared tool
    pub fn shared_tool(mut self, tool: Arc<dyn Tool>) -> Self {
        self.tools.add_shared(tool);
        self
    }

    /// Add multiple tools from a toolset
    pub fn tools(mut self, tools: ToolSet) -> Self {
        for (_, tool) in tools.iter() {
            self.tools.add_shared(Arc::clone(tool));
        }
        self
    }

    /// Build the agent
    pub fn build(self) -> Result<Agent<P>> {
        // Validate configuration
        if self.config.model.is_empty() {
            return Err(Error::agent_config("model name cannot be empty"));
        }

        Ok(Agent {
            provider: Arc::new(self.provider),
            tools: self.tools,
            config: self.config,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_config_default() {
        let config = AgentConfig::default();
        assert_eq!(config.model, "gpt-4o");
        assert!(config.system_prompt.is_none());
        assert_eq!(config.max_tokens, Some(4096));
    }
}
