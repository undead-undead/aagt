//! Memory system for agents
//!
//! Provides short-term (conversation) and long-term (persistent) memory.

use std::collections::VecDeque;
use std::sync::Arc;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};

use crate::message::Message;
use crate::store::file::{FileStore, FileStoreConfig};
use crate::rag::VectorStore;
use std::collections::HashMap;
use std::path::PathBuf;

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
/// Backed by FileStore (JSONL)
pub struct LongTermMemory {
    store: Arc<FileStore>,
    max_entries: usize,
}

impl LongTermMemory {
    /// Create with capacity and path
    pub async fn new(max_entries: usize, path: PathBuf) -> crate::error::Result<Self> {
        let config = FileStoreConfig::new(path);
        let store = Arc::new(FileStore::new(config).await?);
        Ok(Self {
            store,
            max_entries,
        })
    }

    /// Store a memory entry
    pub async fn store_entry(&self, entry: MemoryEntry) -> crate::error::Result<()> {
        let mut metadata = HashMap::new();
        metadata.insert("user_id".to_string(), entry.user_id.clone());
        metadata.insert("timestamp".to_string(), entry.timestamp.to_string());
        metadata.insert("tags".to_string(), serde_json::to_string(&entry.tags).unwrap_or_default());
        metadata.insert("relevance".to_string(), entry.relevance.to_string());

        // We don't strictly enforce max_entries on insert here to avoid high cost of counting/deleting every time.
        // A periodic cleanup task is better.
        
        self.store.store(&entry.content, metadata).await?;
        Ok(())
    }

    /// Retrieve entries by tag
    pub async fn retrieve_by_tag(&self, user_id: &str, tag: &str, limit: usize) -> Vec<MemoryEntry> {
        let uid = user_id.to_string();
        let tag_pat = format!("\"{}\"", tag); // Simple JSON array substring check

        let docs = self.store.find(move |idx| {
            if idx.get_metadata("user_id") != Some(&uid) { return false; }
            if let Some(tags) = idx.get_metadata("tags") {
                tags.contains(&tag_pat)
            } else {
                 false
            }
        }).await;
        
        docs.into_iter()
            .filter_map(|doc| Self::doc_to_entry(doc))
            .take(limit)
            .collect()
    }

    /// Retrieve recent entries with token awareness (approximate)
    pub async fn retrieve_recent(&self, user_id: &str, char_limit: usize) -> Vec<MemoryEntry> {
        let uid = user_id.to_string();
        
        // Find all entries for user (Accesses Index RAM only)
        let docs = self.store.find(move |idx| {
            idx.get_metadata("user_id") == Some(&uid)
        }).await;
        
        let mut user_entries: Vec<MemoryEntry> = docs.into_iter()
            .filter_map(|doc| Self::doc_to_entry(doc))
            .collect();
            
        user_entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp)); // newest first

        let mut result = Vec::new();
        let mut current_chars = 0;

        for entry in user_entries {
            if current_chars + entry.content.len() > char_limit {
                break;
            }
            current_chars += entry.content.len();
            result.push(entry);
        }

        result
    }

    /// Helper to convert Document to MemoryEntry
    fn doc_to_entry(doc: crate::rag::Document) -> Option<MemoryEntry> {
        let user_id = doc.metadata.get("user_id")?.clone();
        let timestamp = doc.metadata.get("timestamp")?.parse().ok()?;
        let tags: Vec<String> = serde_json::from_str(doc.metadata.get("tags")?).ok()?;
        let relevance = doc.metadata.get("relevance")?.parse().ok().unwrap_or(1.0);

        Some(MemoryEntry {
            id: doc.id,
            user_id,
            content: doc.content,
            timestamp,
            tags,
            relevance,
        })
    }

    /// Clear all entries for a user
    pub async fn clear(&self, user_id: &str) {
        // Inefficient but functional for FileStore
        let all = self.store.get_all().await;
        for doc in all {
            if doc.metadata.get("user_id").map(|s| s.as_str()) == Some(user_id) {
                self.store.delete(&doc.id).await.ok();
            }
        }
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
    /// Create with default settings (Async)
    pub async fn new() -> crate::error::Result<Self> {
        Self::with_capacity(100, 1000, PathBuf::from("data/memory.jsonl")).await
    }

    /// Create with custom capacities and path
    pub async fn with_capacity(short_term_max: usize, long_term_max: usize, path: PathBuf) -> crate::error::Result<Self> {
        Ok(Self {
            short_term: Arc::new(ShortTermMemory::new(short_term_max)),
            long_term: Arc::new(LongTermMemory::new(long_term_max, path).await?),
        })
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


    #[tokio::test]
    async fn test_long_term_memory() {
        let path = PathBuf::from("test_memory.jsonl");
        if path.exists() {
            let _ = std::fs::remove_file(&path);
        }
        let memory = LongTermMemory::new(100, path).await.unwrap();

        memory.store_entry(MemoryEntry {
            id: "1".to_string(),
            user_id: "user1".to_string(),
            content: "User prefers SOL".to_string(),
            timestamp: 1000,
            tags: vec!["preference".to_string()],
            relevance: 1.0,
        }).await.unwrap();

        let entries = memory.retrieve_by_tag("user1", "preference", 10).await;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].content, "User prefers SOL");
    }
}
