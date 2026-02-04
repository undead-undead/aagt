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
    /// Policy for risky tools
    pub tool_policy: RiskyToolPolicy,
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
            tool_policy: RiskyToolPolicy::default(),
        }
    }
}

/// Policy for tool execution
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolPolicy {
    /// Allow execution without approval
    Auto,
    /// Require explicit approval
    RequiresApproval,
    /// Disable execution completely
    Disabled,
}

/// Configuration for risky tool policies
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RiskyToolPolicy {
    /// Default policy for all tools
    pub default_policy: ToolPolicy,
    /// Overrides for specific tools
    pub overrides: std::collections::HashMap<String, ToolPolicy>,
}

impl Default for RiskyToolPolicy {
    fn default() -> Self {
        Self {
            default_policy: ToolPolicy::Auto,
            overrides: std::collections::HashMap::new(),
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
    /// Tool execution requires approval
    ApprovalPending { tool: String, input: String },
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
        if let Err(e) = self.events.send(event) {
            // Log warning if no receivers (not critical, but good to know)
            tracing::debug!("Failed to emit event (no receivers): {}", e);
        }
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
    pub async fn chat(&self, mut messages: Vec<Message>) -> Result<String> {
        let mut steps = 0;
        const MAX_STEPS: usize = 15;

        loop {
            if steps >= MAX_STEPS {
                return Err(Error::agent_config("Max agent steps exceeded"));
            }
            steps += 1;

            if let Some(last) = messages.last() {
                 if last.role == Role::User {
                    self.emit(AgentEvent::Thinking { prompt: last.content.as_text() });
                 }
            }

            info!("Agent starting chat completion (step {})", steps);

            let mut stream = self.stream_chat(messages.clone()).await?;
            
            let mut full_text = String::new();
            let mut tool_calls = Vec::new();

            let mut stream_inner = stream.into_inner();

            // Consume the stream
            use futures::StreamExt;
            while let Some(chunk) = stream_inner.next().await {
                match chunk? {
                    crate::streaming::StreamingChoice::Message(text) => {
                        full_text.push_str(&text);
                    }
                    crate::streaming::StreamingChoice::ToolCall { id, name, arguments } => {
                        tool_calls.push((id, name, arguments));
                    }
                     crate::streaming::StreamingChoice::ParallelToolCalls(map) => {
                         // Flatten parallel calls
                         // Map is index -> ToolCall. Order by index.
                         let mut sorted: Vec<_> = map.into_iter().collect();
                         sorted.sort_by_key(|(k,_)| *k);
                         for (_, tc) in sorted {
                             // tc.arguments is passed as Value in streaming choice from OpenAI
                             // But wait, StreamingChoice definition in streaming.rs might differ from my implementation in openai.rs
                             // Let's assume openai.rs implementation (Value) is correct and reflects StreamingChoice
                             tool_calls.push((tc.id, tc.name, tc.arguments));
                         }
                    }
                    _ => {}
                }
            }

            // If no tool calls, we are done
            if tool_calls.is_empty() {
                self.emit(AgentEvent::Response { content: full_text.clone() });
                return Ok(full_text);
            }

            // We have tool calls.
            // 1. Append Assistant Message (Thought + Calls) to history
            let mut parts = Vec::new();
            if !full_text.is_empty() {
                parts.push(crate::message::ContentPart::Text { text: full_text.clone() });
            }
            for (id, name, args) in &tool_calls {
                parts.push(crate::message::ContentPart::ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    arguments: args.clone(),
                });
            }
            messages.push(Message {
                role: Role::Assistant,
                name: None,
                content: Content::Parts(parts),
            });

            // 2. Execute Tools (Parallel)
            // Fix C1: Avoid moving self in async closures
            let tools = &self.tools;
            let policy = &self.config.tool_policy;
            let events = &self.events;
            
            let mut futures = Vec::new();
            
            for (id, name, args) in tool_calls {
                let name_clone = name.clone();
                let id_clone = id.clone();
                let args_str = args.to_string();
                
                futures.push(async move {
                    // Check policy
                    let effective_policy = policy.overrides.get(&name_clone)
                        .unwrap_or(&policy.default_policy);
                    
                    let result = match effective_policy {
                        ToolPolicy::Disabled => {
                            Err(Error::tool_execution(name_clone.clone(), "Tool execution is disabled by policy".to_string()))
                        }
                        ToolPolicy::RequiresApproval => {
                            let _ = events.send(AgentEvent::ApprovalPending { 
                                tool: name_clone.clone(), 
                                input: args_str.clone() 
                            });
                            Err(Error::ToolApprovalRequired { tool_name: name_clone.clone() })
                        }
                        ToolPolicy::Auto => {
                            let _ = events.send(AgentEvent::ToolCall { 
                                tool: name_clone.clone(), 
                                input: args_str.clone() 
                            });
                            tools.call(&name_clone, &args_str).await
                        }
                    };
                    
                    let output = match result {
                        Ok(s) => {
                            let _ = events.send(AgentEvent::ToolResult { 
                                tool: name_clone.clone(), 
                                output: s.clone() 
                            });
                            s
                        }
                        Err(e) => {
                            let _ = events.send(AgentEvent::Error { message: e.to_string() });
                            format!("Error: {}", e)
                        }
                    };

                    (id_clone, output, name_clone)
                });
            }
            
            let results = futures::future::join_all(futures).await;

            // 3. Append Tool Results to history
            // Each result is a separate Tool message
            for (id, output, name) in results {
                 messages.push(Message {
                    role: Role::Tool,
                    name: None, // Optional name on Message, but we can set it in Content if needed
                    content: Content::Parts(vec![crate::message::ContentPart::ToolResult {
                        tool_call_id: id,
                        content: output,
                        name: Some(name),
                    }]),
                });
            }
            
            // Loop continues to generate response based on tool results
        }
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
        // 1. Check Policy
        let policy = self.config.tool_policy.overrides.get(name)
            .unwrap_or(&self.config.tool_policy.default_policy);

        match policy {
            ToolPolicy::Disabled => {
                 return Err(Error::tool_execution(name.to_string(), "Tool execution is disabled by policy".to_string()));
            }
            ToolPolicy::RequiresApproval => {
                self.emit(AgentEvent::ApprovalPending { tool: name.to_string(), input: arguments.to_string() });
                
                // For now, in v1, we block/error if approval is needed because we don't have a resume mechanism yet.
                // In a real TUI/GUI, this would pause or wait on a channel.
                // Here we return a specific error that the caller can catch to prompt user?
                // Actually, let's just error for now to be safe.
                return Err(Error::ToolApprovalRequired { tool_name: name.to_string() });
            }
            ToolPolicy::Auto => {} // Proceed
        }

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

    /// Set tool policy
    pub fn tool_policy(mut self, policy: RiskyToolPolicy) -> Self {
        self.config.tool_policy = policy;
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

        // Fix M4: Increase event channel capacity to avoid message loss in high-frequency scenarios
        let (tx, _) = broadcast::channel(1000);

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
