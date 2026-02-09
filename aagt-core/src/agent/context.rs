//! Context Management Module
//!
//! This module provides the `ContextManager` which is responsible for:
//! - Managing conversation history (short-term memory)
//! - Constructing the final prompt/messages for the LLM
//! - Handling token budgeting and windowing
//! - Injecting system prompts and dynamic context (RAG)

use crate::agent::message::Message;
use crate::error::Result;

/// Configuration for the Context Manager
#[derive(Debug, Clone)]
pub struct ContextConfig {
    /// Maximum tokens allowed in the context window
    pub max_tokens: usize,
    /// Maximum number of messages to keep in history
    pub max_history_messages: usize,
    /// Reserve tokens for the response
    pub response_reserve: usize,
    /// Whether to enable explicit context caching markers
    pub enable_cache_control: bool,
    /// Whether to summarize pruned history
    pub smart_pruning: bool,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            max_tokens: 128000, // Modern default (e.g. GPT-4o)
            max_history_messages: 50,
            response_reserve: 4096,
            enable_cache_control: false,
            smart_pruning: false,
        }
    }
}

/// Trait for injecting dynamic context
#[async_trait::async_trait]
pub trait ContextInjector: Send + Sync {
    /// Generate messages to inject into the context
    async fn inject(&self) -> Result<Vec<Message>>;
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
    /// 1. System prompt injection (Protected)
    /// 2. Dynamic Context Injection (RAG, etc.) (Protected)
    /// 3. Token budgeting using tiktoken (Soft Pruning)
    /// 4. Message windowing (based on max_history_messages)
    pub async fn build_context(&self, history: &[Message]) -> Result<Vec<Message>> {
        // 1. Initialize Tokenizer
        let bpe = tiktoken_rs::cl100k_base().map_err(|e| {
            crate::error::Error::Internal(format!("Failed to load tokenizer: {}", e))
        })?;

        let mut final_context_start = Vec::new();

        // --- 1. System Prompt (Protected) ---
        if let Some(prompt) = &self.system_prompt {
            final_context_start.push(Message::system(prompt.clone()));
        }

        // --- 2. Run Injectors (Protected - e.g. RAG) ---
        // In a more advanced version, we might want to budget RAG too, but for now we treat it as critical context.
        for injector in &self.injectors {
            match injector.inject().await {
                Ok(msgs) => final_context_start.extend(msgs),
                Err(e) => tracing::warn!("Context injector failed: {}", e),
            }
        }

        // --- 3. Calculate Budget ---
        // Safety Margin: 1000 tokens for formatting, JSON overhead, and fragmentation
        const SAFETY_MARGIN: usize = 1000;

        let reserved_response = self.config.response_reserve;
        let max_window = self.config.max_tokens;

        // Calculate current usage from System + RAG
        let mut current_usage = 0;
        for msg in &final_context_start {
            current_usage += bpe.encode_with_special_tokens(&msg.content.as_text()).len();
            current_usage += 4; // Approx per-message overhead
        }

        // Check if we already blew the budget
        let total_reserved = reserved_response + SAFETY_MARGIN + current_usage;
        if total_reserved > max_window {
            tracing::warn!(
                "System prompt + RAG context exceeds context window! (Usage: {}, Limit: {})",
                current_usage,
                max_window - reserved_response - SAFETY_MARGIN
            );
            // We proceed, but truncation is guaranteed.
        }

        let history_budget = if max_window > total_reserved {
            max_window - total_reserved
        } else {
            0
        };

        // --- 4. Select History (Sliding Window & Smart Pruning) ---
        let mut selected_history = Vec::new();
        let mut history_usage = 0;
        let mut pruned_messages = Vec::new();

        let history_slice = if history.len() > self.config.max_history_messages {
            let (pruned, selected) = history.split_at(history.len() - self.config.max_history_messages);
            pruned_messages.extend(pruned.iter().cloned());
            selected
        } else {
            history
        };

        // Iterate REVERSE (Latest first) for selection
        for msg in history_slice.iter().rev() {
            let tokens = bpe.encode_with_special_tokens(&msg.content.as_text()).len();
            let cost = tokens + 4; 

            if history_usage + cost <= history_budget {
                history_usage += cost;
                selected_history.push(msg.clone());
            } else {
                pruned_messages.push(msg.clone());
            }
        }

        // Handle Smart Pruning: Summarize pruned messages into an Observation Log
        if self.config.smart_pruning && !pruned_messages.is_empty() {
             let mut log = String::from("### Historical Observation Log (Pruned Summaries)\n");
             // Pruned messages were collected in reverse or split order, let's sort them roughly by time or just list them
             // For simplicity, we just extract tool calls and key facts
             for msg in pruned_messages {
                 match msg.role {
                     crate::agent::message::Role::Assistant => {
                         let text = msg.content.as_text();
                         let snippet = if text.len() > 60 {
                             format!("{}...", &text[..60].replace('\n', " "))
                         } else {
                             text.replace('\n', " ")
                         };
                         log.push_str(&format!("- Assistant: {}\n", snippet));
                     }
                     crate::agent::message::Role::Tool => {
                         let name = msg.name.as_deref().unwrap_or("unknown_tool");
                         log.push_str(&format!("- Tool executed: {}\n", name));
                     }
                     _ => {}
                 }
             }
             final_context_start.push(Message::system(log));
        }

        // --- 5. Assemble Final Context ---
        let mut final_messages = final_context_start;
        selected_history.reverse();
        final_messages.extend(selected_history);

        Ok(final_messages)
    }

    /// Estimate token count for a list of messages using tiktoken
    pub fn estimate_tokens(messages: &[Message]) -> usize {
        if let Ok(bpe) = tiktoken_rs::cl100k_base() {
            messages
                .iter()
                .map(|m| bpe.encode_with_special_tokens(&m.content.as_text()).len() + 4)
                .sum()
        } else {
            // Fallback to heuristic if tokenizer fails
            messages
                .iter()
                .map(|m| m.content.as_text().len() / 4)
                .sum::<usize>()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // use crate::agent::message::Content;

    #[tokio::test]
    async fn test_smart_pruning_generation() {
        let config = ContextConfig {
            max_history_messages: 2, // Only keep 2 latest messages
            max_tokens: 10000,
            response_reserve: 1000,
            smart_pruning: true,
            ..Default::default()
        };
        let mut mgr = ContextManager::new(config);
        mgr.set_system_prompt("System Prompt");

        let history = vec![
            Message::assistant("I am thinking about the first task."),
            Message::user("What about the second one?"),
            Message::assistant("Executing the third part now."),
            Message::user("Final question."),
        ];

        // Should keep "Executing the third part now." and "Final question."
        // And summarize "I am thinking about the first task." and "What about the second one?"
        let ctx = mgr.build_context(&history).await.unwrap();

        // System Prompt + Observation Log + 2 History Messages = 4 messages
        assert_eq!(ctx.len(), 4, "Context should contain System, Log, and 2 history messages");
        
        let log_msg = &ctx[1];
        assert!(log_msg.content.as_text().contains("Observation Log"), "Should contain Observation Log");
        assert!(log_msg.content.as_text().contains("Assistant"), "Should mention Assistant in log");
    }

    #[tokio::test]
    async fn test_basic_inclusion() {
        let config = ContextConfig::default();
        let mgr = ContextManager::new(config);
        let history = vec![Message::user("test")];
        let ctx = mgr.build_context(&history).await.unwrap();
        // System prompt is None by default, so just history or empty system?
        // Let's check: ContextManager::new initializes system_prompt to None.
        assert!(ctx.len() >= 1);
    }
}
