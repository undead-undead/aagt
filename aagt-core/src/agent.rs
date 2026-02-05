//! Agent system - the core AI agent abstraction

use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{info, instrument};
use anyhow;

use crate::error::{Error, Result};
use crate::message::{Content, Message, Role};
use crate::provider::Provider;
use crate::streaming::StreamingResponse;
use crate::tool::{Tool, ToolSet, memory::{SearchHistoryTool, RememberThisTool}};
use crate::memory::MemoryManager;
use crate::context::{ContextManager, ContextConfig, ContextInjector};

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
    /// Max history messages to send to LLM (Sliding window)
    pub max_history_messages: usize,
    /// Max characters allowed in tool output before truncation
    pub max_tool_output_chars: usize,
    /// Enable strict JSON mode (response_format: json_object)
    pub json_mode: bool,
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
            max_history_messages: 20,
            max_tool_output_chars: 4096,
            json_mode: false,
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

/// Handler for user approvals
#[async_trait::async_trait]
pub trait ApprovalHandler: Send + Sync {
    /// Request approval for a tool call
    async fn approve(&self, tool_name: &str, arguments: &str) -> anyhow::Result<bool>;
}

/// A default approval handler that rejects all
pub struct RejectAllApprovalHandler;

#[async_trait::async_trait]
impl ApprovalHandler for RejectAllApprovalHandler {
    async fn approve(&self, _tool: &str, _args: &str) -> anyhow::Result<bool> {
        Ok(false)
    }
}

/// Request sent to the channel handler
#[derive(Debug)]
pub struct ApprovalRequest {
    /// Unique ID for this request
    pub id: String,
    /// Tool name
    pub tool_name: String,
    /// Tool arguments
    pub arguments: String,
    /// Responder channel
    pub responder: tokio::sync::oneshot::Sender<bool>,
}

/// A handler that sends approval requests via a channel
pub struct ChannelApprovalHandler {
    sender: tokio::sync::mpsc::Sender<ApprovalRequest>,
}

impl ChannelApprovalHandler {
    /// Create a new channel handler
    pub fn new(sender: tokio::sync::mpsc::Sender<ApprovalRequest>) -> Self {
        Self { sender }
    }
}

#[async_trait::async_trait]
impl ApprovalHandler for ChannelApprovalHandler {
    async fn approve(&self, tool_name: &str, arguments: &str) -> anyhow::Result<bool> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        
        let request = ApprovalRequest {
            id: uuid::Uuid::new_v4().to_string(),
            tool_name: tool_name.to_string(),
            arguments: arguments.to_string(),
            responder: tx,
        };

        self.sender.send(request).await
            .map_err(|_| Error::Internal("Approval channel closed".to_string()))?;

        // Wait for response
        let approved = rx.await
            .map_err(|_| Error::Internal("Approval responder dropped".to_string()))?;
            
        Ok(approved)
    }
}

use crate::notification::{Notifier, NotifyChannel};

/// The main Agent struct
pub struct Agent<P: Provider> {
    provider: Arc<P>,
    tools: ToolSet,
    config: AgentConfig,
    context_manager: ContextManager,
    events: broadcast::Sender<AgentEvent>,
    approval_handler: Arc<dyn ApprovalHandler>,
    notifier: Option<Arc<dyn Notifier>>,
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
            tracing::debug!("Failed to emit event (no receivers): {}", e);
        }
    }
    
    /// Send a notification via the configured notifier
    pub async fn notify(&self, channel: NotifyChannel, message: &str) -> Result<()> {
        if let Some(notifier) = &self.notifier {
             notifier.notify(channel, message).await
        } else {
             // If no notifier configured, log warning but don't fail hard
             tracing::warn!("Agent tried to notify but no notifier is configured: {}", message);
             Ok(())
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

            // Context Window Management via ContextManager
            let context_messages = self.context_manager.build_context(&messages)
                .map_err(|e| Error::agent_config(format!("Failed to build context: {}", e)))?;

            let stream = self.stream_chat(context_messages).await?;
            
            let mut full_text = String::new();
            let mut tool_calls = Vec::new(); // (id, name, args)

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
                         let mut sorted: Vec<_> = map.into_iter().collect();
                         sorted.sort_by_key(|(k,_)| *k);
                         for (_, tc) in sorted {
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
            let tools = &self.tools;
            let policy = &self.config.tool_policy;
            let events = &self.events;
            let approval_handler = &self.approval_handler;
            
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
                            
                            // Ask approval handler
                            match approval_handler.approve(&name_clone, &args_str).await {
                                Ok(true) => {
                                    // Approved! Proceed to auto
                                    let _ = events.send(AgentEvent::ToolCall { 
                                        tool: name_clone.clone(), 
                                        input: args_str.clone() 
                                    });
                                    tools.call(&name_clone, &args_str).await
                                        .map_err(|e| Error::tool_execution(name_clone.clone(), e.to_string()))
                                }
                                Ok(false) => {
                                    Err(Error::ToolApprovalRequired { tool_name: name_clone.clone() })
                                }
                                Err(e) => {
                                    Err(Error::tool_execution(name_clone.clone(), format!("Approval check failed: {}", e)))
                                }
                            }
                        }
                        ToolPolicy::Auto => {
                            let _ = events.send(AgentEvent::ToolCall { 
                                tool: name_clone.clone(), 
                                input: args_str.clone() 
                            });
                            // internal tool call returns anyhow::Result
                            tools.call(&name_clone, &args_str).await
                                .map_err(|e| Error::tool_execution(name_clone.clone(), e.to_string()))
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
            for (id, output, name) in results {
                 messages.push(Message {
                    role: Role::Tool,
                    name: None,
                    content: Content::Parts(vec![crate::message::ContentPart::ToolResult {
                        tool_call_id: id,
                        content: output,
                        name: Some(name),
                    }]),
                });
            }
        }
    }

    /// Stream a prompt response
    pub async fn stream(&self, prompt: impl Into<String>) -> Result<StreamingResponse> {
        let messages = vec![Message::user(prompt.into())];
        self.stream_chat(messages).await
    }

    /// Stream a chat response
    pub async fn stream_chat(&self, messages: Vec<Message>) -> Result<StreamingResponse> {
        let mut extra = self.config.extra_params.clone().unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
        
        // Inject JSON mode if enabled
        if self.config.json_mode {
            if let serde_json::Value::Object(ref mut map) = extra {
                if !map.contains_key("response_format") {
                     map.insert("response_format".to_string(), serde_json::json!({ "type": "json_object" }));
                }
            }
        }

        self.provider
            .stream_completion(
                &self.config.model,
                Some(&self.config.preamble), // Use preamble as system prompt
                messages,
                self.tools.definitions().await,
                self.config.temperature,
                self.config.max_tokens,
                Some(extra),
            )
            .await
    }

    /// Call a tool by name (Direct call helper)
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
                
                match self.approval_handler.approve(name, arguments).await {
                    Ok(true) => {}, // Proceed
                    Ok(false) => return Err(Error::ToolApprovalRequired { tool_name: name.to_string() }),
                    Err(e) => return Err(Error::tool_execution(name.to_string(), format!("Approval check failed: {}", e)))
                }
            }
            ToolPolicy::Auto => {} // Proceed
        }

        self.emit(AgentEvent::ToolCall { tool: name.to_string(), input: arguments.to_string() });

        let result = self.tools.call(name, arguments).await;
        
        match result {
            Ok(mut output) => {
                // Quota Protection: Truncate tool output if too long
                if output.len() > self.config.max_tool_output_chars {
                    let original_len = output.len();
                    output.truncate(self.config.max_tool_output_chars);
                    output.push_str(&format!("\n\n(Note: Output truncated from {} to {} chars to save tokens)", 
                        original_len, self.config.max_tool_output_chars));
                }

                self.emit(AgentEvent::ToolResult { tool: name.to_string(), output: output.clone() });
                Ok(output)
            },
            Err(e) => {
                self.emit(AgentEvent::Error { message: e.to_string() });
                // Map anyhow error to ToolExecution error
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
    injectors: Vec<Box<dyn ContextInjector>>,
    approval_handler: Option<Arc<dyn ApprovalHandler>>,
    notifier: Option<Arc<dyn Notifier>>,
}

impl<P: Provider> AgentBuilder<P> {
    /// Create a new builder with a provider
    pub fn new(provider: P) -> Self {
        Self {
            provider,
            tools: ToolSet::new(),
            config: AgentConfig::default(),
            injectors: Vec::new(),
            approval_handler: None,
            notifier: None,
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

    /// Set external approval handler
    pub fn approval_handler(mut self, handler: impl ApprovalHandler + 'static) -> Self {
        self.approval_handler = Some(Arc::new(handler));
        self
    }

    /// Set max history messages (sliding window)
    pub fn max_history_messages(mut self, count: usize) -> Self {
        self.config.max_history_messages = count;
        self
    }

    /// Set max tool output characters
    pub fn max_tool_output_chars(mut self, count: usize) -> Self {
        self.config.max_tool_output_chars = count;
        self
    }

    /// Enable strict JSON mode (enforces response_format: json_object)
    pub fn json_mode(mut self, enable: bool) -> Self {
        self.config.json_mode = enable;
        self
    }
    
    /// Set a notifier
    pub fn notifier(mut self, notifier: impl Notifier + 'static) -> Self {
        self.notifier = Some(Arc::new(notifier));
        self
    }

    /// Add a context injector
    pub fn context_injector(mut self, injector: impl ContextInjector + 'static) -> Self {
        self.injectors.push(Box::new(injector));
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

    /// Add memory tools using the provided memory manager
    pub fn with_memory(mut self, memory: Arc<MemoryManager>) -> Self {
        let engine = memory.long_term.engine();
        
        // 1. Add tools to the agent
        self.tools.add(SearchHistoryTool::new(engine.clone()));
        self.tools.add(RememberThisTool::new(engine.clone()));
        
        self
    }

    /// Add code interpreter capability using the given sidecar address
    pub async fn with_code_interpreter(mut self, address: impl Into<String>) -> Result<Self> {
        let sidecar = crate::capabilities::Sidecar::connect(address.into()).await?;
        let shared_sidecar = Arc::new(tokio::sync::Mutex::new(sidecar));
        
        self.tools.add(crate::tool::code_interpreter::CodeInterpreter::new(shared_sidecar));
        
        Ok(self)
    }

    /// Build the agent
    pub fn build(self) -> Result<Agent<P>> {
        // Validate configuration
        if self.config.model.is_empty() {
            return Err(Error::agent_config("model name cannot be empty"));
        }
        if self.config.max_history_messages == 0 {
            return Err(Error::agent_config("max_history_messages must be at least 1"));
        }

        let (tx, _) = broadcast::channel(1000);

        let mut context_config = ContextConfig::default();
        context_config.max_history_messages = self.config.max_history_messages;
        if let Some(tokens) = self.config.max_tokens {
            // Rough heuristic: Context window is usually larger than max_tokens (generation limit)
            // But we don't have model context window size in config yet.
            // For now, let's just ensure we respect max_history_messages primarily.
            context_config.response_reserve = tokens as usize;
        }

        let mut context_manager = ContextManager::new(context_config);
        context_manager.set_system_prompt(self.config.preamble.clone());
        
        for injector in self.injectors {
            context_manager.add_injector(injector);
        }

        Ok(Agent {
            provider: Arc::new(self.provider),
            tools: self.tools,
            config: self.config,
            context_manager,
            events: tx,
            approval_handler: self.approval_handler.unwrap_or_else(|| Arc::new(RejectAllApprovalHandler)),
            notifier: self.notifier,
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
