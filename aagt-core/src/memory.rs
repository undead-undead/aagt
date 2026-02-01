//! Memory system for agents
//!
//! Provides short-term (conversation) and long-term (persistent) memory.

use std::collections::VecDeque;
use std::sync::Arc;

use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::message::Message;

/// Trait for memory implementations
pub trait Memory: Send + Sync {
    /// Store a message
    fn store(&self, user_id: &str, message: Message);

    /// Retrieve recent messages
    fn retrieve(&self, user_id: &str, limit: usize) -> Vec<Message>;

    /// Clear memory for a user
    fn clear(&self, user_id: &str);
}

/// Short-term memory - stores recent conversation history
/// Uses a fixed-size ring buffer per user for memory efficiency
pub struct ShortTermMemory {
    /// Max messages to keep per user
    max_messages: usize,
    /// Storage: user_id -> message ring buffer
    store: DashMap<String, VecDeque<Message>>,
}

impl ShortTermMemory {
    /// Create with custom capacity
    pub fn new(max_messages: usize) -> Self {
        Self {
            max_messages,
            store: DashMap::new(),
        }
    }

    /// Create with default capacity (100 messages per user)
    pub fn default_capacity() -> Self {
        Self::new(100)
    }

    /// Get current message count for a user
    pub fn message_count(&self, user_id: &str) -> usize {
        self.store.get(user_id).map(|v| v.len()).unwrap_or(0)
    }
}

impl Memory for ShortTermMemory {
    fn store(&self, user_id: &str, message: Message) {
        let mut entry = self.store.entry(user_id.to_string()).or_default();

        // Ring buffer behavior: remove oldest if at capacity
        if entry.len() >= self.max_messages {
            entry.pop_front();
        }
        entry.push_back(message);
    }

    fn retrieve(&self, user_id: &str, limit: usize) -> Vec<Message> {
        self.store
            .get(user_id)
            .map(|v| {
                let skip = v.len().saturating_sub(limit);
                v.iter().skip(skip).cloned().collect()
            })
            .unwrap_or_default()
    }

    fn clear(&self, user_id: &str) {
        self.store.remove(user_id);
    }
}

/// Memory entry for long-term storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    /// Unique ID
    pub id: String,
    /// User ID this belongs to
    pub user_id: String,
    /// Content/summary
    pub content: String,
    /// Timestamp
    pub timestamp: i64,
    /// Tags for categorization
    pub tags: Vec<String>,
    /// Relevance score (for retrieval ranking)
    pub relevance: f32,
}

/// Long-term memory - stores important information persistently
/// This is an in-memory implementation; for production, extend with DB backend
pub struct LongTermMemory {
    /// Storage: user_id -> entries
    store: DashMap<String, Vec<MemoryEntry>>,
    /// Max entries per user
    max_entries: usize,
}

impl LongTermMemory {
    /// Create with capacity
    pub fn new(max_entries: usize) -> Self {
        Self {
            store: DashMap::new(),
            max_entries,
        }
    }

    /// Store a memory entry
    pub fn store_entry(&self, entry: MemoryEntry) {
        let mut entries = self.store.entry(entry.user_id.clone()).or_default();

        // Remove oldest if at capacity
        if entries.len() >= self.max_entries {
            // Optimization: Find index of oldest entry instead of full sort
            if let Some((min_idx, _)) = entries.iter().enumerate().min_by_key(|(_, e)| e.timestamp)
            {
                entries.remove(min_idx);
            }
        }
        entries.push(entry);
    }

    /// Retrieve entries by tag
    pub fn retrieve_by_tag(&self, user_id: &str, tag: &str, limit: usize) -> Vec<MemoryEntry> {
        self.store
            .get(user_id)
            .map(|entries| {
                entries
                    .iter()
                    .filter(|e| e.tags.contains(&tag.to_string()))
                    .take(limit)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Retrieve recent entries with token awareness (approximate)
    /// Limit is roughly the max characters allowed (char_limit) NOT count of entries
    pub fn retrieve_recent(&self, user_id: &str, char_limit: usize) -> Vec<MemoryEntry> {
        self.store
            .get(user_id)
            .map(|entries| {
                let mut sorted: Vec<_> = entries.iter().cloned().collect();
                sorted.sort_by(|a, b| b.timestamp.cmp(&a.timestamp)); // newest first

                let mut result = Vec::new();
                let mut current_chars = 0;

                for entry in sorted {
                    if current_chars + entry.content.len() > char_limit {
                        break;
                    }
                    current_chars += entry.content.len();
                    result.push(entry);
                }

                result
            })
            .unwrap_or_default()
    }

    /// Clear all entries for a user
    pub fn clear(&self, user_id: &str) {
        self.store.remove(user_id);
    }

    /// Get entry count for a user
    pub fn entry_count(&self, user_id: &str) -> usize {
        self.store.get(user_id).map(|v| v.len()).unwrap_or(0)
    }

    /// Clean up inactive users (Optional manual GC)
    pub fn cleanup_users(&self, active_user_ids: &[String]) {
        self.store.retain(|k, _| active_user_ids.contains(k));
    }
}

/// Combined memory manager
pub struct MemoryManager {
    /// Short-term conversation memory
    pub short_term: Arc<ShortTermMemory>,
    /// Long-term persistent memory
    pub long_term: Arc<LongTermMemory>,
}

impl MemoryManager {
    /// Create with default settings
    pub fn new() -> Self {
        Self {
            short_term: Arc::new(ShortTermMemory::default_capacity()),
            long_term: Arc::new(LongTermMemory::new(1000)),
        }
    }

    /// Create with custom capacities
    pub fn with_capacity(short_term_max: usize, long_term_max: usize) -> Self {
        Self {
            short_term: Arc::new(ShortTermMemory::new(short_term_max)),
            long_term: Arc::new(LongTermMemory::new(long_term_max)),
        }
    }
}

impl Default for MemoryManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_short_term_memory() {
        let memory = ShortTermMemory::new(3);

        memory.store("user1", Message::user("Hello"));
        memory.store("user1", Message::assistant("Hi there"));
        memory.store("user1", Message::user("How are you?"));
        memory.store("user1", Message::assistant("I'm good!")); // This should evict "Hello"

        let messages = memory.retrieve("user1", 10);
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].text(), "Hi there");
    }

    #[test]
    fn test_long_term_memory() {
        let memory = LongTermMemory::new(100);

        memory.store_entry(MemoryEntry {
            id: "1".to_string(),
            user_id: "user1".to_string(),
            content: "User prefers SOL".to_string(),
            timestamp: 1000,
            tags: vec!["preference".to_string()],
            relevance: 1.0,
        });

        let entries = memory.retrieve_by_tag("user1", "preference", 10);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].content, "User prefers SOL");
    }
}
