//! Agent system - the core AI agent abstraction

use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{info, instrument};

use crate::error::{Error, Result};
use crate::message::{Content, Message, Role};
use crate::provider::Provider;
use crate::streaming::StreamingResponse;
use crate::tool::{Tool, ToolSet};

/// Configuration for an Agent
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// Name of the agent (for logging/identity)
    pub name: String,
    /// Model to use (provider specific string)
    pub model: String,
    /// System prompt / Preamble
    pub preamble: String,
    /// Temperature for generation
    pub temperature: Option<f64>,
    /// Max tokens to generate
    pub max_tokens: Option<u64>,
    /// Additional provider-specific parameters
    pub extra_params: Option<serde_json::Value>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            name: "agent".to_string(),
            model: "gpt-4o".to_string(),
            preamble: "You are a helpful AI assistant.".to_string(),
            temperature: Some(0.7),
            max_tokens: Some(4096),
            extra_params: None,
        }
    }
}

/// Events emitted by the Agent during execution
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum AgentEvent {
    /// Agent started thinking (prompt received)
    Thinking { prompt: String },
    /// Agent decided to use a tool
    ToolCall { tool: String, input: String },
    /// Tool execution finished
    ToolResult { tool: String, output: String },
    /// Agent generated a final response
    Response { content: String },
    /// Error occurred
    Error { message: String },
}

/// The main Agent struct
pub struct Agent<P: Provider> {
    provider: Arc<P>,
    tools: ToolSet,
    config: AgentConfig,
    /// Event bus for observability
    events: broadcast::Sender<AgentEvent>,
}

impl<P: Provider> Agent<P> {
    /// Create a new agent builder
    pub fn builder(provider: P) -> AgentBuilder<P> {
        AgentBuilder::new(provider)
    }

    /// Subscribe to agent events
    pub fn subscribe(&self) -> broadcast::Receiver<AgentEvent> {
        self.events.subscribe()
    }

    /// Helper to emit events safely
    fn emit(&self, event: AgentEvent) {
        let _ = self.events.send(event);
    }

    /// Send a prompt and get a response (non-streaming)
    #[instrument(skip(self, prompt), fields(model = %self.config.model))]
    pub async fn prompt(&self, prompt: impl Into<String>) -> Result<String> {
        let prompt_str = prompt.into();
        self.emit(AgentEvent::Thinking { prompt: prompt_str.clone() });
        
        let messages = vec![
            Message::user(prompt_str)
        ];
        
        self.chat(messages).await
    }

    /// Send messages and get a response (non-streaming)
    #[instrument(skip(self, messages), fields(model = %self.config.model, message_count = messages.len()))]
    pub async fn chat(&self, messages: Vec<Message>) -> Result<String> {
        if let Some(last) = messages.last() {
             // Only emit thinking if not already emitted by prompt()
             if last.role == Role::User {
                // We blindly emit thinking here for chat calls too
                self.emit(AgentEvent::Thinking { prompt: last.content.as_text() });
             }
        }

        info!("Agent starting chat completion");

        let stream = self.stream_chat(messages).await?;
        
        // Collect stream response
        // Note: collecting text consumes the stream.
        // Ideally we would want to emit tokens as they come, but for now we emit full response at end.
        let response = stream.collect_text().await?;

        self.emit(AgentEvent::Response { content: response.clone() });
        Ok(response)
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
                Some(&self.config.preamble), // Use preamble as system prompt
                messages,
                self.tools.definitions().await,
                self.config.temperature,
                self.config.max_tokens,
                self.config.extra_params.clone(),
            )
            .await
    }

    /// Call a tool by name
    #[instrument(skip(self, arguments), fields(tool_name = %name))]
    pub async fn call_tool(&self, name: &str, arguments: &str) -> Result<String> {
        self.emit(AgentEvent::ToolCall { tool: name.to_string(), input: arguments.to_string() });

        let result = self.tools.call(name, arguments).await;
        
        match result {
            Ok(ref output) => {
                self.emit(AgentEvent::ToolResult { tool: name.to_string(), output: output.clone() });
                Ok(output.clone())
            },
            Err(ref e) => {
                self.emit(AgentEvent::Error { message: e.to_string() });
                Err(Error::tool_execution(name.to_string(), e.to_string()))
            }
        }
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
        self.config.preamble = prompt.into();
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

        let (tx, _) = broadcast::channel(100);

        Ok(Agent {
            provider: Arc::new(self.provider),
            tools: self.tools,
            config: self.config,
            events: tx,
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
        assert_eq!(config.max_tokens, Some(4096));
    }
}
