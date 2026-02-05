//! Context Management Module
//!
//! This module provides the `ContextManager` which is responsible for:
//! - Managing conversation history (short-term memory)
//! - Constructing the final prompt/messages for the LLM
//! - Handling token budgeting and windowing
//! - Injecting system prompts and dynamic context (RAG)

use crate::error::Result;
use crate::message::{Message, Role};

/// Configuration for the Context Manager
#[derive(Debug, Clone)]
pub struct ContextConfig {
    /// Maximum tokens allowed in the context window
    pub max_tokens: usize,
    /// Maximum number of messages to keep in history
    pub max_history_messages: usize,
    /// Reserve tokens for the response
    pub response_reserve: usize,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            max_tokens: 128000, // Modern default (e.g. GPT-4o)
            max_history_messages: 50,
            response_reserve: 4096,
        }
    }
}

/// Trait for injecting dynamic context
pub trait ContextInjector: Send + Sync {
    /// Generate messages to inject into the context
    fn inject(&self) -> Result<Vec<Message>>;
}

/// Manages the context window for an agent
pub struct ContextManager {
    config: ContextConfig,
    system_prompt: Option<String>,
    injectors: Vec<Box<dyn ContextInjector>>,
}

impl ContextManager {
    /// Create a new ContextManager
    pub fn new(config: ContextConfig) -> Self {
        Self {
            config,
            system_prompt: None,
            injectors: Vec::new(),
        }
    }

    /// Set the system prompt
    pub fn set_system_prompt(&mut self, prompt: impl Into<String>) {
        self.system_prompt = Some(prompt.into());
    }

    /// Add a context injector
    pub fn add_injector(&mut self, injector: Box<dyn ContextInjector>) {
        self.injectors.push(injector);
    }

    /// Construct the final list of messages to send to the provider
    ///
    /// This method applies:
    /// 1. System prompt injection
    /// 2. Dynamic Context Injection (RAG, etc.)
    /// 3. Message windowing (based on max_history_messages)
    /// 4. Token budgeting (truncating old messages if needed - FUTURE)
    pub fn build_context(&self, history: &[Message]) -> Result<Vec<Message>> {
        let mut final_messages = Vec::new();

        // 1. Add System Prompt
        if let Some(prompt) = &self.system_prompt {
            final_messages.push(Message::system(prompt.clone()));
        }

        // 2. Run Injectors
        for injector in &self.injectors {
            match injector.inject() {
                Ok(msgs) => final_messages.extend(msgs),
                Err(e) => tracing::warn!("Context injector failed: {}", e),
            }
        }

        // 3. Select recent history
        // Simple windowing for now
        let history_start = if history.len() > self.config.max_history_messages {
            history.len() - self.config.max_history_messages
        } else {
            0
        };

        let recent_history = &history[history_start..];
        final_messages.extend_from_slice(recent_history);

        // 4. Token Check (Placeholder)
        // In the future, we would iterate backwards and check token counts here.

        Ok(final_messages)
    }

    /// Estimate token count for a list of messages
    ///
    /// This is a rough heuristic (chars / 4).
    /// For precise counting, we need a tokenizer (e.g. tiktoken).
    pub fn estimate_tokens(messages: &[Message]) -> usize {
        messages
            .iter()
            .map(|m| m.content.as_text().len())
            .sum::<usize>()
            / 4
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::Content;

    #[test]
    fn test_context_windowing() {
        let config = ContextConfig {
            max_history_messages: 2,
            ..Default::default()
        };
        let mut mgr = ContextManager::new(config);
        mgr.set_system_prompt("System");

        let history = vec![Message::user("1"), Message::user("2"), Message::user("3")];

        let ctx = mgr.build_context(&history).unwrap();

        // Should contain System + Last 2 messages
        assert_eq!(ctx.len(), 3);
        assert_eq!(ctx[0].role, Role::System);
        assert_eq!(ctx[1].text(), "2");
        assert_eq!(ctx[2].text(), "3");
    }
}
