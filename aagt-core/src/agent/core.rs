//! Agent system - the core AI agent abstraction

use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{info, instrument, error, debug};
use anyhow;

use crate::error::{Error, Result};
use crate::agent::context::ContextInjector;
use crate::agent::message::{Message, Role, Content};
use crate::agent::provider::Provider;
use crate::agent::memory::Memory;
use crate::agent::session::SessionStatus;
use crate::skills::tool::{Tool, ToolSet};
use crate::agent::streaming::StreamingResponse;
use crate::skills::tool::memory::{SearchHistoryTool, RememberThisTool, TieredSearchTool, FetchDocumentTool}; // Corrected import for memory tools
use crate::agent::context::{ContextManager, ContextConfig}; // ContextInjector is already imported above
use crate::agent::multi_agent::{Coordinator, AgentRole, MultiAgent, AgentMessage};
use crate::agent::personality::{Persona, PersonalityManager};
use crate::agent::cache::Cache;
use crate::agent::scheduler::Scheduler;
use crate::skills::tool::{DelegateTool, CronTool};
use crate::infra::notification::{Notifier, NotifyChannel};

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
    /// Optional personality profile
    pub persona: Option<Persona>,
    /// Role of the agent in a multi-agent system
    pub role: AgentRole,
    /// Max parallel tool calls (default: 5)
    pub max_parallel_tools: usize,
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
            persona: None,
            role: AgentRole::Assistant,
            max_parallel_tools: 5,
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

/// Trait for human-in-the-loop interactions (getting text input)
#[async_trait::async_trait]
pub trait InteractionHandler: Send + Sync {
    /// Ask the user a question and get a string response
    async fn ask(&self, question: &str) -> anyhow::Result<String>;
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct AskUserArgs {
    /// The question to ask the user
    question: String,
}

struct AskUserTool {
    handler: Arc<dyn InteractionHandler>,
}

#[async_trait::async_trait]
impl crate::skills::tool::Tool for AskUserTool {
    fn name(&self) -> String {
        "ask_user".to_string()
    }

    async fn definition(&self) -> crate::skills::tool::ToolDefinition {
        let gen = schemars::gen::SchemaSettings::openapi3().into_generator();
        let schema = gen.into_root_schema_for::<AskUserArgs>();
        let schema_json = serde_json::to_value(schema).unwrap_or_default();

        crate::skills::tool::ToolDefinition {
            name: "ask_user".to_string(),
            description: "Ask the user for clarification, additional information, or a final decision. Use this when you are stuck or need human input.".to_string(),
            parameters: schema_json,
            parameters_ts: Some("interface AskUserArgs {\n  /** The question to ask the user */\n  question: string;\n}".to_string()),
            is_binary: false,
            is_verified: true,
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let args: AskUserArgs = serde_json::from_str(arguments)?;
        self.handler.ask(&args.question).await
    }
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

// use crate::infra::notification::{Notifier, NotifyChannel}; // Already imported at top

/// The main Agent struct
pub struct Agent<P: Provider> {
    provider: Arc<P>,
    tools: ToolSet,
    config: AgentConfig,
    context_manager: ContextManager,
    events: broadcast::Sender<AgentEvent>,
    approval_handler: Arc<dyn ApprovalHandler>,
    cache: Option<Arc<dyn Cache>>,
    notifier: Option<Arc<dyn Notifier>>,
    memory: Option<Arc<dyn Memory>>,
    session_id: Option<String>,
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

    /// Save current state to persistent storage
    pub async fn checkpoint(&self, messages: &[Message], step: usize, status: SessionStatus) -> Result<()> {
        if let (Some(memory), Some(session_id)) = (&self.memory, &self.session_id) {
            let session = crate::agent::session::AgentSession {
                id: session_id.clone(),
                messages: messages.to_vec(),
                step,
                status,
                updated_at: chrono::Utc::now(),
            };
            memory.store_session(session).await?;
            debug!("Agent checkpoint saved for session: {}", session_id);
        }
        Ok(())
    }

    /// Resume a previously saved session
    pub async fn resume(&self, session_id: &str) -> Result<String> {
        if let Some(memory) = &self.memory {
            if let Some(session) = memory.retrieve_session(session_id).await? {
                info!("Resuming agent session: {}", session_id);
                // We restart the chat with the loaded messages
                return self.chat(session.messages).await;
            }
        }
        Err(Error::Internal(format!("Session not found: {}", session_id)))
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

            // Save checkpoint before thinking
            self.checkpoint(&messages, steps, SessionStatus::Thinking).await?;

            info!("Agent starting chat completion (step {})", steps);

            // 1. Check Cache (Step-level caching)
            if let Some(cache) = &self.cache {
                if let Ok(Some(cached_response)) = cache.get(&messages).await {
                    info!("Cache hit! Returning cached response.");
                    return Ok(cached_response);
                }
            }

            // Context Window Management via ContextManager
            let context_messages = self.context_manager.build_context(&messages).await
                .map_err(|e| Error::agent_config(format!("Failed to build context: {}", e)))?;

            let stream = self.stream_chat(context_messages).await?;
            
            let mut full_text = String::new();
            let mut tool_calls = Vec::new(); // (id, name, args)

            let mut stream_inner = stream.into_inner();

            // Consume the stream
            use futures::StreamExt;
            while let Some(chunk) = stream_inner.next().await {
                match chunk? {
                    crate::agent::streaming::StreamingChoice::Message(text) => {
                        full_text.push_str(&text);
                    }
                    crate::agent::streaming::StreamingChoice::ToolCall { id, name, arguments } => {
                        tool_calls.push((id, name, arguments));
                    }
                     crate::agent::streaming::StreamingChoice::ParallelToolCalls(map) => {
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
                
                // Store in cache
                if let Some(cache) = &self.cache {
                    let _ = cache.set(&messages, full_text.clone()).await;
                }
                
                return Ok(full_text);
            }

            // We have tool calls.
            // 1. Append Assistant Message (Thought + Calls) to history
            let mut parts = Vec::new();
            if !full_text.is_empty() {
                parts.push(crate::agent::message::ContentPart::Text { text: full_text.clone() });
            }
            for (id, name, args) in &tool_calls {
                parts.push(crate::agent::message::ContentPart::ToolCall {
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

            // 2. Execute Tools (Parallel with Limit)
            let tools = &self.tools;
            let policy = &self.config.tool_policy;
            let events = &self.events;
            let approval_handler = &self.approval_handler;
            let max_parallel = self.config.max_parallel_tools;
            
            use futures::stream;
            
            let current_messages = Arc::new(messages.clone());
            
            let results: Vec<crate::error::Result<(String, String, String)>> = stream::iter(tool_calls)
                .map(|(id, name, args)| {
                    let name_clone = name.clone();
                    let id_clone = id.clone();
                    let args_str = args.to_string();
                    let msgs = Arc::clone(&current_messages);
                    
                    async move {
                        // 1. Get tool definition (cached in ToolSet)
                        let tool_ref = tools.get(&name_clone).ok_or_else(|| Error::ToolNotFound(name_clone.clone()))?;
                        
                        let def = tool_ref.definition().await;

                        // 2. Check policy and security overrides
                        let mut effective_policy = policy.overrides.get(&name_clone)
                            .unwrap_or(&policy.default_policy).clone();
                        
                        // Binary Safety Override: Unverified binary skills ALWAYS require approval
                        if def.is_binary && !def.is_verified {
                            if effective_policy != ToolPolicy::Disabled {
                                tracing::warn!(tool = %name_clone, "Unverified binary skill detected. Enforcing manual approval.");
                                effective_policy = ToolPolicy::RequiresApproval;
                            }
                        }

                        let result = match effective_policy {
                            ToolPolicy::Disabled => {
                                Err(Error::tool_execution(name_clone.clone(), "Tool execution is disabled by policy".to_string()))
                            }
                            ToolPolicy::RequiresApproval => {
                                let _ = events.send(AgentEvent::ApprovalPending { 
                                    tool: name_clone.clone(), 
                                    input: args_str.clone() 
                                });
                                
                                // Checkpoint before awaiting approval
                                self.checkpoint(&msgs, steps, SessionStatus::AwaitingApproval { 
                                    tool_name: name_clone.clone(), 
                                    arguments: args_str.clone() 
                                }).await?;

                                // Ask approval handler
                                match approval_handler.approve(&name_clone, &args_str).await {
                                    Ok(true) => {
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
                                tools.call(&name_clone, &args_str).await
                                    .map_err(|e| Error::tool_execution(name_clone.clone(), e.to_string()))
                            }
                        };
                        
                        match result {
                            Ok(output) => {
                                let _ = events.send(AgentEvent::ToolResult { 
                                    tool: name_clone.clone(), 
                                    output: output.clone() 
                                });
                                Ok((id_clone, name_clone, output))
                            },
                            Err(e) => {
                                let _ = events.send(AgentEvent::Error { message: e.to_string() });
                                Ok((id_clone, name_clone, format!("Error: {}", e)))
                            }
                        }
                    }
                })
                .buffer_unordered(max_parallel)
                .collect()
                .await;

            // 3. Append Tool Results to history
            for res in results {
                let (id, name, output) = res.unwrap(); // Safe because we handle Err inside async move
                 messages.push(Message {
                    role: Role::Tool,
                    name: None,
                    content: Content::Parts(vec![crate::agent::message::ContentPart::ToolResult {
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

        let request = crate::agent::provider::ChatRequest {
            model: self.config.model.clone(),
            system_prompt: Some(self.config.preamble.clone()),
            messages,
            tools: self.tools.definitions().await,
            temperature: self.config.temperature,
            max_tokens: self.config.max_tokens,
            extra_params: Some(extra),
        };

        self.provider.stream_completion(request).await
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

    /// Add tool definitions
    pub async fn tool_definitions(&self) -> Vec<crate::skills::tool::ToolDefinition> {
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

    /// Start a proactive loop that listens for tasks from multiple sources
    pub async fn listen(
        &self, 
        mut user_input: tokio::sync::mpsc::Receiver<String>,
        mut external_events: tokio::sync::mpsc::Receiver<AgentMessage>
    ) -> Result<()> {
        info!("Agent {} starting proactive loop", self.config.name);
        
        loop {
            tokio::select! {
                // 1. Handle user input
                input = user_input.recv() => {
                    match input {
                        Some(text) => {
                            if let Err(e) = self.process(&text).await {
                                error!("Error in proactive user task: {}", e);
                            }
                        }
                        None => {
                            info!("User input channel closed, exiting proactive loop");
                            break;
                        }
                    }
                }
                
                // 2. Handle external agent/system messages (e.g. from Scheduler)
                msg = external_events.recv() => {
                    match msg {
                        Some(message) => {
                            if let Err(e) = self.handle_message(message).await {
                                error!("Error in proactive external task: {}", e);
                            }
                        }
                        None => {
                            info!("External events channel closed, exiting proactive loop");
                            break;
                        }
                    }
                }
            }
        }
        
        Ok(())
    }
}

/// Builder for creating agents
pub struct AgentBuilder<P: Provider> {
    provider: P,
    tools: ToolSet,
    config: AgentConfig,
    injectors: Vec<Box<dyn ContextInjector>>,
    approval_handler: Option<Arc<dyn ApprovalHandler>>,
    interaction_handler: Option<Arc<dyn InteractionHandler>>,
    notifier: Option<Arc<dyn Notifier>>,
    cache: Option<Arc<dyn Cache>>,
    /// Security: Track if Python Sidecar is enabled (mutually exclusive with DynamicSkill)
    has_sidecar: bool,
    /// Security: Track if DynamicSkill is enabled (mutually exclusive with Sidecar)
    has_dynamic_skill: bool,
    memory: Option<Arc<dyn Memory>>,
    session_id: Option<String>,
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
            interaction_handler: None,
            notifier: None,
            cache: None,
            has_sidecar: false,
            has_dynamic_skill: false,
            memory: None,
            session_id: None,
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

    /// Set interaction handler (for HITL)
    pub fn interaction_handler(mut self, handler: impl InteractionHandler + 'static) -> Self {
        self.interaction_handler = Some(Arc::new(handler));
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
    
    /// Set the agent's personality
    pub fn persona(mut self, persona: Persona) -> Self {
        self.config.persona = Some(persona);
        self
    }
    
    /// Set a notifier
    pub fn notifier(mut self, notifier: impl Notifier + 'static) -> Self {
        self.notifier = Some(Arc::new(notifier));
        self
    }

    /// Set session ID for persistence
    pub fn session_id(mut self, id: impl Into<String>) -> Self {
        self.session_id = Some(id.into());
        self
    }

    /// Set the agent's role
    pub fn role(mut self, role: AgentRole) -> Self {
        self.config.role = role;
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

    /// Add memory tools using the provided memory implementation
    pub fn with_memory(mut self, memory: Arc<dyn crate::agent::memory::Memory>) -> Self {
        self.tools.add(SearchHistoryTool::new(memory.clone()));
        self.tools.add(RememberThisTool::new(memory.clone()));
        self.tools.add(TieredSearchTool::new(memory.clone()));
        self.tools.add(FetchDocumentTool::new(memory.clone()));
        
        self.memory = Some(memory);
        self
    }

    /// Add DynamicSkill support (ClawHub skills, custom scripts)
    /// 
    /// # Security
    /// 
    /// **CRITICAL**: DynamicSkill and Python Sidecar are mutually exclusive.
    /// This method will return an error if Python Sidecar has already been configured.
    /// 
    /// **Rationale**: If both are enabled, malicious DynamicSkills can pollute the
    /// Agent's context with secrets, which may then be used by LLM-generated Python
    /// code in the unsandboxed Sidecar to exfiltrate data.
    /// 
    /// See SECURITY.md for details.
    pub fn with_dynamic_skills(mut self, skill_loader: Arc<crate::skills::SkillLoader>) -> Result<Self> {
        // Security check: prevent enabling both Sidecar and DynamicSkill
        if self.has_sidecar {
            return Err(Error::agent_config(
                "Security Error: Cannot enable DynamicSkill when Python Sidecar is configured. \
                These are mutually exclusive due to context pollution risks. \
                See SECURITY.md for details."
            ));
        }
        
        // Add all loaded skills as tools
        for skill_ref in skill_loader.skills.iter() {
            self.tools.add_shared(Arc::clone(skill_ref.value()) as Arc<dyn crate::skills::tool::Tool>);
        }
        
        // Add ClawHub and ReadSkillDoc tools
        self.tools.add(crate::skills::ClawHubTool::new(Arc::clone(&skill_loader)));
        self.tools.add(crate::skills::ReadSkillDoc::new(skill_loader));
        
        self.has_dynamic_skill = true;
        
        Ok(self)
    }

    /// Add code interpreter capability using the given sidecar address
    /// 
    /// # Security
    /// 
    /// **CRITICAL**: Python Sidecar and DynamicSkill are mutually exclusive.
    /// This method will return an error if DynamicSkill has already been configured.
    /// 
    /// **Rationale**: Python Sidecar has no sandbox isolation. If DynamicSkill is also
    /// enabled, malicious skills can pollute the Agent's context, leading to secret
    /// exfiltration via LLM-generated Python code in the Sidecar.
    /// 
    /// See SECURITY.md for details.
    pub async fn with_code_interpreter(mut self, address: impl Into<String>) -> Result<Self> {
        // Security check: prevent enabling both Sidecar and DynamicSkill
        if self.has_dynamic_skill {
            return Err(Error::agent_config(
                "Security Error: Cannot enable Python Sidecar when DynamicSkill is configured. \
                These are mutually exclusive due to context pollution risks. \
                See SECURITY.md for details."
            ));
        }
        
        let sidecar = crate::skills::capabilities::Sidecar::connect(address.into()).await?;
        let shared_sidecar = Arc::new(tokio::sync::Mutex::new(sidecar));
        
        self.tools.add(crate::skills::tool::code_interpreter::CodeInterpreter::new(shared_sidecar));
        self.has_sidecar = true;
        
        Ok(self)
    }

    /// Build the agent
    /// 
    /// # Security Defaults
    /// 
    /// If neither Python Sidecar nor DynamicSkill has been explicitly configured,
    /// this method will automatically enable DynamicSkill with default settings:
    /// - Skills directory: `./skills`
    /// - Network access: disabled (secure sandbox)
    /// 
    /// To use Python Sidecar instead, call `.with_code_interpreter()` before `.build()`.
    pub fn build(mut self) -> Result<Agent<P>> {
        // Validate configuration
        if self.config.model.is_empty() {
            return Err(Error::agent_config("model name cannot be empty"));
        }
        if self.config.max_history_messages == 0 {
            return Err(Error::agent_config("max_history_messages must be at least 1"));
        }

        // SECURITY DEFAULT: Auto-enable DynamicSkill if no execution model configured
        if !self.has_sidecar && !self.has_dynamic_skill {
            info!("No execution model configured. Auto-enabling DynamicSkill (default)...");
            
            // Try to load skills from default directory
            let skill_loader = Arc::new(crate::skills::SkillLoader::new("./skills"));
            
            // Attempt to load skills (non-fatal if directory doesn't exist)
            match tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(skill_loader.load_all())
            }) {
                Ok(_) => {
                    info!("Loaded DynamicSkills from ./skills");
                    
                    // Add all loaded skills as tools
                    for skill_ref in skill_loader.skills.iter() {
                        self.tools.add_shared(Arc::clone(skill_ref.value()) as Arc<dyn crate::skills::tool::Tool>);
                    }
                    
                    // Add ClawHub and ReadSkillDoc tools
                    self.tools.add(crate::skills::ClawHubTool::new(Arc::clone(&skill_loader)));
                    self.tools.add(crate::skills::ReadSkillDoc::new(skill_loader));
                    
                    self.has_dynamic_skill = true;
                },
                Err(e) => {
                    // Non-fatal: Skills directory doesn't exist or is empty
                    info!("DynamicSkill auto-enable skipped (no skills found): {}", e);
                    // Continue without skills - agent will still function with other tools
                }
            }
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
        
        // Inject all tools as TS interfaces in the system prompt
        // This fulfills the 'Replace JSON with TS in Prompt' requirement.
        context_manager.add_injector(Box::new(self.tools.clone()));

        for injector in self.injectors {
            context_manager.add_injector(injector);
        }

        if let Some(persona) = &self.config.persona {
            context_manager.add_injector(Box::new(PersonalityManager::new(persona.clone())));
        }

        // Auto-register AskUser tool if handler available
        let mut tools = self.tools;
        if let Some(handler) = &self.interaction_handler {
            tools.add(AskUserTool { handler: Arc::clone(handler) });
        }

        Ok(Agent {
            provider: Arc::new(self.provider),
            tools,
            config: self.config,
            context_manager,
            events: tx,
            approval_handler: self.approval_handler.unwrap_or_else(|| Arc::new(RejectAllApprovalHandler)),
            cache: self.cache,
            notifier: self.notifier,
            memory: self.memory,
            session_id: self.session_id,
        })
    }

    /// Add delegation support using the provided coordinator
    pub fn with_delegation(mut self, coordinator: Arc<Coordinator>) -> Self {
        self.tools.add(DelegateTool::new(Arc::downgrade(&coordinator)));
        self
    }

    /// Add scheduling support using the provided scheduler
    pub fn with_scheduler(mut self, scheduler: Arc<Scheduler>) -> Self {
        self.tools.add(CronTool::new(Arc::downgrade(&scheduler)));
        self
    }
}

#[async_trait::async_trait]
impl<P: Provider> MultiAgent for Agent<P> {
    fn role(&self) -> AgentRole {
        self.config.role.clone()
    }

    async fn handle_message(&self, message: AgentMessage) -> Result<Option<AgentMessage>> {
        info!("Agent {:?} handling message from {:?}", self.role(), message.from);
        let response = self.prompt(message.content).await?;
        
        Ok(Some(AgentMessage {
            from: self.role(),
            to: Some(message.from),
            content: response,
            msg_type: crate::agent::multi_agent::MessageType::Response,
        }))
    }

    async fn process(&self, input: &str) -> Result<String> {
        self.prompt(input).await
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
