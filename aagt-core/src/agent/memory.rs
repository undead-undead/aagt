//! Memory system for agents
//!
//! Provides short-term (conversation) and long-term (persistent) memory.

use std::collections::VecDeque;
use std::sync::Arc;

use dashmap::DashMap;

use crate::agent::message::Message;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Weak;
use async_trait::async_trait;

use crate::agent::scheduler::Scheduler;

/// Trait for memory implementations
#[async_trait]
pub trait Memory: Send + Sync {
    /// Store a message
    async fn store(&self, user_id: &str, agent_id: Option<&str>, message: Message) -> crate::error::Result<()>;

    /// Store multiple messages efficiently
    async fn store_batch(&self, user_id: &str, agent_id: Option<&str>, messages: Vec<Message>) -> crate::error::Result<()> {
        for msg in messages {
            self.store(user_id, agent_id, msg).await?;
        }
        Ok(())
    }

    /// Retrieve recent messages
    async fn retrieve(&self, user_id: &str, agent_id: Option<&str>, limit: usize) -> Vec<Message>;

    /// Search the memory for relevant content
    async fn search(&self, user_id: &str, agent_id: Option<&str>, query: &str, limit: usize) -> crate::error::Result<Vec<crate::knowledge::rag::Document>> {
        let _ = (user_id, agent_id, query, limit);
        Ok(Vec::new())
    }

    /// Store a specific piece of knowledge (not just a message)
    async fn store_knowledge(&self, user_id: &str, agent_id: Option<&str>, title: &str, content: &str, collection: &str) -> crate::error::Result<()> {
        let _ = (user_id, agent_id, title, content, collection);
        Ok(())
    }

    /// Clear memory for a user
    async fn clear(&self, user_id: &str, agent_id: Option<&str>) -> crate::error::Result<()>;

    /// Undo last message
    async fn undo(&self, user_id: &str, agent_id: Option<&str>) -> crate::error::Result<Option<Message>>;

    /// Update summary for a piece of knowledge
    async fn update_summary(&self, collection: &str, path: &str, summary: &str) -> crate::error::Result<()> {
        let _ = (collection, path, summary);
        Ok(())
    }

    /// Link a scheduler for background tasks
    fn link_scheduler(&self, _scheduler: Weak<Scheduler>) {}

    /// Fetch a full document by path
    async fn fetch_document(&self, collection: &str, path: &str) -> crate::error::Result<Option<crate::knowledge::rag::Document>> {
        let _ = (collection, path);
        Ok(None)
    }

    /// Store an agent session state
    async fn store_session(&self, _session: crate::agent::session::AgentSession) -> crate::error::Result<()> {
        Ok(())
    }

    /// Retrieve an agent session state
    async fn retrieve_session(&self, _session_id: &str) -> crate::error::Result<Option<crate::agent::session::AgentSession>> {
        Ok(None)
    }
}

/// Short-term memory - stores recent conversation history
/// Uses a fixed-size ring buffer per user for memory efficiency
/// Persists to disk (JSON) to allow restarts without losing context.
pub struct ShortTermMemory {
    /// Max messages to keep per user
    max_messages: usize,
    /// Max active users/contexts to keep in memory (DoS protection)
    max_users: usize,
    /// Storage: composite_key -> message ring buffer
    store: DashMap<String, VecDeque<Message>>,
    /// Track last access time for cleanup
    last_access: DashMap<String, std::time::Instant>,
    /// Persistence path
    path: PathBuf,
}

impl ShortTermMemory {
    /// Create with custom capacity and persistence path
    pub async fn new(max_messages: usize, max_users: usize, path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let store = DashMap::new();
        let last_access = DashMap::new();
        
        let mem = Self {
            max_messages,
            max_users,
            store,
            last_access,
            path,
        };
        
        // Try to load existing state
        if let Err(e) = mem.load().await {
            tracing::warn!("Failed to load short-term memory from {:?}: {}", mem.path, e);
        }
        
        mem
    }

    /// Create with default capacity (100 messages per user, 1000 active users)
    pub async fn default_capacity() -> Self {
        Self::new(100, 1000, "data/short_term_memory.json").await
    }

    /// Load state from disk
    async fn load(&self) -> crate::error::Result<()> {
        if !self.path.exists() {
            return Ok(());
        }
        
        let content = tokio::fs::read_to_string(&self.path).await
            .map_err(|e| crate::error::Error::Internal(format!("Failed to read memory file: {}", e)))?;
            
        if content.trim().is_empty() {
            return Ok(());
        }

        let data: HashMap<String, VecDeque<Message>> = serde_json::from_str(&content)
            .map_err(|e| crate::error::Error::Internal(format!("Failed to parse memory file: {}", e)))?;
            
        self.store.clear();
        for (k, v) in data {
            self.store.insert(k.clone(), v);
            self.last_access.insert(k, std::time::Instant::now());
        }
        
        tracing::info!("Loaded short-term memory for {} users", self.store.len());
        Ok(())
    }

    /// Save state to disk
    async fn save(&self) -> crate::error::Result<()> {
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent).await.ok();
        }
        
        // Convert DashMap to HashMap for serialization
        let data: HashMap<_, _> = self.store.iter().map(|r| (r.key().clone(), r.value().clone())).collect();
        
        let json = serde_json::to_string_pretty(&data)
             .map_err(|e| crate::error::Error::Internal(format!("Failed to serialize memory: {}", e)))?;
             
        // Atomic save: write to tmp then rename
        let tmp_path = self.path.with_extension("tmp");
        tokio::fs::write(&tmp_path, json).await
             .map_err(|e| crate::error::Error::Internal(format!("Failed to write temporary memory file: {}", e)))?;
             
        tokio::fs::rename(tmp_path, &self.path).await
             .map_err(|e| crate::error::Error::Internal(format!("Failed to rename memory file: {}", e)))?;
             
        Ok(())
    }

    /// Get current message count for a user/agent pair
    pub fn message_count(&self, user_id: &str, agent_id: Option<&str>) -> usize {
        let key = self.key(user_id, agent_id);
        self.store.get(&key).map(|v| v.len()).unwrap_or(0)
    }
    
    /// Generate composite key
    fn key(&self, user_id: &str, agent_id: Option<&str>) -> String {
        if let Some(agent) = agent_id {
            format!("{}:{}", user_id, agent)
        } else {
            user_id.to_string()
        }
    }
    
    /// Prune inactive users (older than duration) - Useful for manual cleanup
    pub fn prune_inactive(&self, duration: std::time::Duration) {
        let now = std::time::Instant::now();
        // DashMap retain is efficient
        self.last_access.retain(|key, last_time| {
            let keep = now.duration_since(*last_time) < duration;
            if !keep {
                self.store.remove(key);
            }
            keep
        });
    }

    /// Check and enforce total user capacity (LRU eviction)
    fn enforce_user_capacity(&self) {
        if self.store.len() < self.max_users {
            return;
        }

        let mut oldest_key = None;
        let mut oldest_time = std::time::Instant::now();

        for r in self.last_access.iter() {
            if *r.value() < oldest_time {
                oldest_time = *r.value();
                oldest_key = Some(r.key().clone());
            }
        }

        if let Some(key) = oldest_key {
            self.store.remove(&key);
            self.last_access.remove(&key);
        }
    }

    /// Pop the oldest N messages for a user
    pub async fn pop_oldest(&self, user_id: &str, agent_id: Option<&str>, count: usize) -> Vec<Message> {
        let key = self.key(user_id, agent_id);
        let mut popped = Vec::new();
        
        if let Some(mut entry) = self.store.get_mut(&key) {
             for _ in 0..count {
                 if let Some(msg) = entry.pop_front() {
                     popped.push(msg);
                 } else {
                     break;
                 }
             }
        }
        
        if !popped.is_empty() {
             // Save change immediately
             let _ = self.save().await;
        }
        
        popped
    }
}

#[async_trait]
impl Memory for ShortTermMemory {
    async fn store(&self, user_id: &str, agent_id: Option<&str>, message: Message) -> crate::error::Result<()> {
        let key = self.key(user_id, agent_id);
        
        // Enforce capacity before inserting new user
        if !self.store.contains_key(&key) {
             self.enforce_user_capacity();
        }

        {
            let mut entry = self.store.entry(key.clone()).or_default();

            // Ring buffer behavior: remove oldest if at capacity
            // NOTE: With Tiered Storage, MemoryManager should handle archiving BEFORE this limit is hit commonly.
            // But as a safety net, we still keep the hard limit.
            if entry.len() >= self.max_messages {
                entry.pop_front();
            }
            entry.push_back(message);
        } // Lock on DashMap bucket dropped here
        
        // Update access time
        self.last_access.insert(key, std::time::Instant::now());
        
        // Save immediately for safety (Async I/O)
        // With Tiered storage, this file stays small (KB), so atomic write is fast enough.
        if let Err(e) = self.save().await {
            tracing::error!("Failed to persist short-term memory: {}", e);
        }

        Ok(())
    }



    async fn retrieve(&self, user_id: &str, agent_id: Option<&str>, limit: usize) -> Vec<Message> {
        let key = self.key(user_id, agent_id);
        self.store
            .get(&key)
            .map(|v| {
                // Update access time on retrieval too
                self.last_access.insert(key, std::time::Instant::now());
                
                let skip = v.len().saturating_sub(limit);
                v.iter().skip(skip).cloned().collect()
            })
            .unwrap_or_default()
    }

    async fn store_knowledge(&self, user_id: &str, agent_id: Option<&str>, title: &str, content: &str, collection: &str) -> crate::error::Result<()> {
        let text = format!("[{}] {}: {}", collection, title, content);
        self.store(user_id, agent_id, Message::assistant(text)).await
    }

    async fn clear(&self, user_id: &str, agent_id: Option<&str>) -> crate::error::Result<()> {
        let key = self.key(user_id, agent_id);
        self.store.remove(&key);
        self.last_access.remove(&key);
        
        self.save().await
    }

    async fn undo(&self, user_id: &str, agent_id: Option<&str>) -> crate::error::Result<Option<Message>> {
        let key = self.key(user_id, agent_id);
        let msg = {
            let mut entry = self.store.entry(key.clone()).or_default();
            entry.pop_back()
        };
        
        if msg.is_some() {
            self.save().await?;
        }
        
        Ok(msg)
    }

    async fn search(&self, user_id: &str, agent_id: Option<&str>, query: &str, limit: usize) -> crate::error::Result<Vec<crate::knowledge::rag::Document>> {
        let query_lower = query.to_lowercase();
        let messages = self.retrieve(user_id, agent_id, 1000).await; // Search through all STM for this user
        
        let mut results = Vec::new();
        for (i, msg) in messages.iter().enumerate() {
            let content = msg.text();
            if content.to_lowercase().contains(&query_lower) {
                results.push(crate::knowledge::rag::Document {
                    id: format!("stm_{}_{}", self.key(user_id, agent_id), i),
                    title: format!("Recent conversation ({})", msg.role.as_str()),
                    content: content.to_string(),
                    summary: None,
                    collection: None,
                    path: None,
                    metadata: HashMap::new(),
                    score: 0.9, // STM matches are highly relevant but given a fixed sub-1.0 score to prioritize exact LTM matches if needed
                });
            }
            if results.len() >= limit {
                break;
            }
        }
        
        Ok(results)
    }
}

/// Combined memory manager for tiered storage
pub struct MemoryManager {
    /// Hot Storage Layer (e.g. In-memory or fast local cache)
    pub hot_tier: Arc<dyn Memory>,
    /// Cold Storage Layer (e.g. SQLite, Vector DB)
    pub cold_tier: Arc<dyn Memory>,
}

impl MemoryManager {
    /// Create a new MemoryManager with specific backends
    pub fn new(hot_tier: Arc<dyn Memory>, cold_tier: Arc<dyn Memory>) -> Self {
        Self { hot_tier, cold_tier }
    }

    /// Tiered Storage Store
    /// Stores in Hot Tier, then auto-archives to Cold Tier if capacity exceeded
    pub async fn store(&self, user_id: &str, agent_id: Option<&str>, message: Message) -> crate::error::Result<()> {
        // 1. Write to Hot Storage - Fast
        self.hot_tier.store(user_id, agent_id, message).await?;
        
        // 2. Archive older messages if needed
        // Note: The specific logic for "when to archive" could be moved to a TieringPolicy
        // For now, we use a simple heuristic if the Hot Tier supports counting.
        // Since we are now using dyn Memory, we might need to add a 'count' method to the trait 
        // if we want generic tiering logic here, or let the Hot Tier handle its own overflow.
        
        Ok(())
    }
    
    /// Unified Retrieve
    /// Fetches from Hot + Cold seamlessly
    pub async fn retrieve_unified(&self, user_id: &str, agent_id: Option<&str>, limit: usize) -> Vec<Message> {
        let mut messages = self.hot_tier.retrieve(user_id, agent_id, limit).await;
        
        if messages.len() < limit {
             let needed = limit - messages.len();
             let cold_messages = self.cold_tier.retrieve(user_id, agent_id, needed).await;
             
             let mut combined = cold_messages;
             combined.extend(messages);
             messages = combined;
        }
        
        messages
    }

    /// Union Search - searches both Hot and Cold tiers
    pub async fn search_unified(&self, user_id: &str, agent_id: Option<&str>, query: &str, limit: usize) -> crate::error::Result<Vec<crate::knowledge::rag::Document>> {
        let hot_results = self.hot_tier.search(user_id, agent_id, query, limit).await?;
        let cold_results = self.cold_tier.search(user_id, agent_id, query, limit).await?;
        
        let mut combined = hot_results;
        for cold_res in cold_results {
            if !combined.iter().any(|r| r.content == cold_res.content) {
                combined.push(cold_res);
            }
        }
        
        combined.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        combined.truncate(limit);
        
        Ok(combined)
    }

    /// Undo last message
    pub async fn undo(&self, user_id: &str, agent_id: Option<&str>) -> crate::error::Result<Option<Message>> {
        let hot_msg = self.hot_tier.undo(user_id, agent_id).await?;
        let _ = self.cold_tier.undo(user_id, agent_id).await?;
        Ok(hot_msg)
    }
}

#[async_trait]
impl Memory for MemoryManager {
    async fn store(&self, user_id: &str, agent_id: Option<&str>, message: Message) -> crate::error::Result<()> {
        self.store(user_id, agent_id, message).await
    }

    async fn retrieve(&self, user_id: &str, agent_id: Option<&str>, limit: usize) -> Vec<Message> {
        self.retrieve_unified(user_id, agent_id, limit).await
    }

    async fn search(&self, user_id: &str, agent_id: Option<&str>, query: &str, limit: usize) -> crate::error::Result<Vec<crate::knowledge::rag::Document>> {
        self.search_unified(user_id, agent_id, query, limit).await
    }

    async fn store_knowledge(&self, user_id: &str, agent_id: Option<&str>, title: &str, content: &str, collection: &str) -> crate::error::Result<()> {
        // Knowledge usually goes directly to Cold tier for permanence
        self.cold_tier.store_knowledge(user_id, agent_id, title, content, collection).await
    }

    async fn clear(&self, user_id: &str, agent_id: Option<&str>) -> crate::error::Result<()> {
        self.hot_tier.clear(user_id, agent_id).await?;
        self.cold_tier.clear(user_id, agent_id).await?;
        Ok(())
    }

    async fn undo(&self, user_id: &str, agent_id: Option<&str>) -> crate::error::Result<Option<Message>> {
        self.undo(user_id, agent_id).await
    }

    async fn store_session(&self, session: crate::agent::session::AgentSession) -> crate::error::Result<()> {
        self.cold_tier.store_session(session).await
    }

    async fn retrieve_session(&self, session_id: &str) -> crate::error::Result<Option<crate::agent::session::AgentSession>> {
        self.cold_tier.retrieve_session(session_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_short_term_memory() {
        let memory = ShortTermMemory::new(3, 10, "test_stm.json").await;

        memory.store("user1", None, Message::user("Hello")).await.unwrap();
        memory.store("user1", None, Message::assistant("Hi there")).await.unwrap();
        memory.store("user1", None, Message::user("How are you?")).await.unwrap();
        // This should evict "Hello"
        memory.store("user1", None, Message::assistant("I'm good!")).await.unwrap(); 

        let messages = memory.retrieve("user1", None, 10).await;
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].text(), "Hi there");
        
        let _ = std::fs::remove_file("test_stm.json");
    }
}
