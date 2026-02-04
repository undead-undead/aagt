//! Memory system for agents
//!
//! Provides short-term (conversation) and long-term (persistent) memory.

use std::collections::VecDeque;
use std::sync::Arc;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};

use crate::message::{Message, Role, Content};
use crate::store::file::{FileStore, FileStoreConfig};
use crate::rag::VectorStore;
use std::collections::HashMap;
use std::path::PathBuf;
use async_trait::async_trait;

/// Trait for memory implementations
#[async_trait]
pub trait Memory: Send + Sync {
    /// Store a message
    async fn store(&self, user_id: &str, agent_id: Option<&str>, message: Message) -> crate::error::Result<()>;

    /// Retrieve recent messages
    async fn retrieve(&self, user_id: &str, agent_id: Option<&str>, limit: usize) -> Vec<Message>;

    /// Clear memory for a user
    async fn clear(&self, user_id: &str, agent_id: Option<&str>) -> crate::error::Result<()>;
}

/// Short-term memory - stores recent conversation history
/// Uses a fixed-size ring buffer per user for memory efficiency
pub struct ShortTermMemory {
    /// Max messages to keep per user
    max_messages: usize,
    /// Max active users/contexts to keep in memory (DoS protection)
    max_users: usize,
    /// Storage: composite_key -> message ring buffer
    store: DashMap<String, VecDeque<Message>>,
    /// Track last access time for cleanup
    last_access: DashMap<String, std::time::Instant>,
}

impl ShortTermMemory {
    /// Create with custom capacity
    pub fn new(max_messages: usize, max_users: usize) -> Self {
        Self {
            max_messages,
            max_users,
            store: DashMap::new(),
            last_access: DashMap::new(),
        }
    }

    /// Create with default capacity (100 messages per user, 1000 active users)
    pub fn default_capacity() -> Self {
        Self::new(100, 1000)
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
    /// Prune inactive users (older than duration)
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
        // Fix: If we are AT capacity (or over), we must evict before adding a new one.
        if self.store.len() < self.max_users {
            return;
        }

        // Simple LRU: Find oldest access time and remove
        // Ideally we would use a more specialized LRU cache data structure, 
        // but iterating DashMap random sample or finding min is generic enough for "personal use" protection.
        
        // Optimization: Just remove a few random if we are way over, or find oldest.
        // For strict correctness, we find oldest.
        // Note: iter() on DashMap can be expensive if huge, but we only do this when full.
        
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
}

#[async_trait]
impl Memory for ShortTermMemory {
    async fn store(&self, user_id: &str, agent_id: Option<&str>, message: Message) -> crate::error::Result<()> {
        let key = self.key(user_id, agent_id);
        
        // Enforce capacity before inserting new user
        if !self.store.contains_key(&key) {
             self.enforce_user_capacity();
        }

        let mut entry = self.store.entry(key.clone()).or_default();

        // Ring buffer behavior: remove oldest if at capacity
        if entry.len() >= self.max_messages {
            entry.pop_front();
        }
        entry.push_back(message);
        
        // Update access time
        self.last_access.insert(key, std::time::Instant::now());

        // Probabilistic cleanup (1% chance)
        if fastrand::usize(..100) == 0 {
             self.prune_inactive(std::time::Duration::from_secs(3600)); // 1 hour timeout
        }

        Ok(())
    }


    async fn retrieve(&self, user_id: &str, agent_id: Option<&str>, limit: usize) -> Vec<Message> {
        let key = self.key(user_id, agent_id);
        self.store
            .get(&key)
            .map(|v| {
                // Update access time
                self.last_access.insert(key, std::time::Instant::now());
                
                let skip = v.len().saturating_sub(limit);
                v.iter().skip(skip).cloned().collect()
            })
            .unwrap_or_default()
    }

    async fn clear(&self, user_id: &str, agent_id: Option<&str>) -> crate::error::Result<()> {
        let key = self.key(user_id, agent_id);
        self.store.remove(&key);
        self.last_access.remove(&key);
        Ok(())
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
    pub async fn store_entry(&self, entry: MemoryEntry, agent_id: Option<&str>) -> crate::error::Result<()> {
        let mut metadata = HashMap::new();
        metadata.insert("user_id".to_string(), entry.user_id.clone());
        if let Some(aid) = agent_id {
            metadata.insert("agent_id".to_string(), aid.to_string());
        }
        metadata.insert("timestamp".to_string(), entry.timestamp.to_string());
        metadata.insert("tags".to_string(), serde_json::to_string(&entry.tags).unwrap_or_default());
        metadata.insert("relevance".to_string(), entry.relevance.to_string());

        // We don't strictly enforce max_entries on insert here to avoid high cost of counting/deleting every time.
        // A periodic cleanup task is better.
        
        self.store.store(&entry.content, metadata).await?;
        
        // Fix #2: Probabilistic Cleanup
        // To avoid overhead on every write, check cleanup 5% of the time
        // or just rely on a separate task. Here we do simple probabilistic check.
        if fastrand::usize(..100) < 5 {
            self.prune(self.max_entries, entry.user_id.clone(), agent_id.map(|s| s.to_string())).await;
        }
        
        Ok(())
    }

    /// Prune old entries if exceeding limit
    pub async fn prune(&self, limit: usize, user_id: String, agent_id: Option<String>) {
        let store = self.store.clone();
        let uid = user_id.clone();
        let aid = agent_id.clone();
        
        tokio::spawn(async move {
            // Find all IDs for this user/agent
            let uid_filter = uid.clone();
            let aid_filter = aid.clone();
            
            // We need timestamp to sort, so we can't just use find_ids.
            // We need to fetch metadata at least.
            // FileStore::find returns Documents which contain metadata.
            // Optimized: Use find_metadata to avoid hydrating content
            let docs = store.find_metadata(move |idx| {
                if idx.get_metadata("user_id") != Some(&uid_filter) { return false; }
                if let Some(ref target_aid) = aid_filter {
                    if idx.get_metadata("agent_id") != Some(target_aid) { return false; }
                }
                true
            }).await;

            if docs.len() <= limit {
                return;
            }

            // Parse timestamps and sort
            let mut entries: Vec<(String, i64)> = docs.into_iter().filter_map(|idx| {
                let ts = idx.get_metadata("timestamp")?.parse::<i64>().ok()?;
                Some((idx.id, ts))
            }).collect();

            // Sort: Oldest first (ascending timestamp)
            entries.sort_by_key(|k| k.1);

            // Determine how many to delete
            let to_remove = entries.len().saturating_sub(limit);
            if to_remove > 0 {
                // Delete oldest
                for (id, _) in entries.into_iter().take(to_remove) {
                    store.delete(&id).await.ok();
                }
                // Trigger compaction if we deleted a lot
                if to_remove > 100 {
                     store.auto_compact(limit).await.ok();
                }
            }
        });
    }

    /// Retrieve entries by tag with optional agent isolation
    pub async fn retrieve_by_tag(&self, user_id: &str, tag: &str, agent_id: Option<&str>, limit: usize) -> Vec<MemoryEntry> {
        let uid = user_id.to_string();
        let tag_to_find = tag.to_string();
        let aid = agent_id.map(|s| s.to_string());

        let docs = self.store.find(move |idx| {
            if idx.get_metadata("user_id") != Some(&uid) { return false; }
            if let Some(ref target_aid) = aid {
                if idx.get_metadata("agent_id") != Some(target_aid) { return false; }
            }
            if let Some(tags_json) = idx.get_metadata("tags") {
                // Precise JSON matching
                let tags: Vec<String> = serde_json::from_str(tags_json).unwrap_or_default();
                tags.contains(&tag_to_find)
            } else {
                 false
            }
        }).await;
        
        docs.into_iter()
            .filter_map(|doc| Self::doc_to_entry(doc))
            .take(limit)
            .collect()
    }

    /// Retrieve recent entries with token awareness (approximate) and optional agent isolation
    pub async fn retrieve_recent(&self, user_id: &str, agent_id: Option<&str>, char_limit: usize) -> Vec<MemoryEntry> {
        let uid = user_id.to_string();
        let aid = agent_id.map(|s| s.to_string());
        
        // Find all entries for user (Accesses Index RAM only)
        let docs = self.store.find(move |idx| {
            if idx.get_metadata("user_id") != Some(&uid) { return false; }
            if let Some(ref target_aid) = aid {
                if idx.get_metadata("agent_id") != Some(target_aid) { return false; }
            }
            true
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
    // Inefficient clear removed in favor of trait implementation
    pub async fn clear_deprecated(&self, user_id: &str, agent_id: Option<&str>) {
        let _ = <Self as Memory>::clear(self, user_id, agent_id).await;
    }
}

#[async_trait]
impl Memory for LongTermMemory {
    async fn store(&self, user_id: &str, agent_id: Option<&str>, message: Message) -> crate::error::Result<()> {
         let entry = MemoryEntry {
             id: uuid::Uuid::new_v4().to_string(),
             user_id: user_id.to_string(),
             content: message.content.as_text(),
             timestamp: chrono::Utc::now().timestamp_millis(),
             tags: vec![message.role.as_str().to_string(), "conversation".to_string()],
             relevance: 1.0, 
         };
         
         // Fix #4: Await the result, no background spawn
         self.store_entry(entry, agent_id).await
    }

    async fn retrieve(&self, user_id: &str, agent_id: Option<&str>, limit: usize) -> Vec<Message> {
         // Map Entry -> Message
         let entries = self.retrieve_recent(user_id, agent_id, limit * 100).await; // char limit approximate
         entries.into_iter().map(|e| {
             let role = if e.tags.contains(&"user".to_string()) { Role::User } else { Role::Assistant };
             Message {
                 role,
                 name: None,
                 content: Content::Text(e.content),
             }
         }).take(limit).collect()
    }
    
    async fn clear(&self, user_id: &str, agent_id: Option<&str>) -> crate::error::Result<()> {
        let uid = user_id.to_string();
        let aid = agent_id.map(|s| s.to_string());
        
        let store = self.store.clone();
        
        // Find IDs first
        let ids_to_delete = store.find_ids(move |idx| {
            if idx.get_metadata("user_id") != Some(&uid) { return false; }
            if let Some(ref target_aid) = aid {
                 if idx.get_metadata("agent_id") != Some(target_aid) { return false; }
            }
            true
        }).await;

        for id in ids_to_delete {
            store.delete(&id).await?;
        }
        Ok(())
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
        Self::with_capacity(100, 1000, 1000, PathBuf::from("data/memory.jsonl")).await
    }

    /// Create with custom capacities and path
    pub async fn with_capacity(short_term_max: usize, short_term_users: usize, long_term_max: usize, path: PathBuf) -> crate::error::Result<Self> {
        Ok(Self {
            short_term: Arc::new(ShortTermMemory::new(short_term_max, short_term_users)),
            long_term: Arc::new(LongTermMemory::new(long_term_max, path).await?),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_short_term_memory() {
        let memory = ShortTermMemory::new(3, 10);

        memory.store("user1", None, Message::user("Hello"));
        memory.store("user1", None, Message::assistant("Hi there"));
        memory.store("user1", None, Message::user("How are you?"));
        memory.store("user1", None, Message::assistant("I'm good!")); // This should evict "Hello"

        let messages = memory.retrieve("user1", None, 10);
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
        }, None).await.unwrap();


        let entries = memory.retrieve_by_tag("user1", "preference", None, 10).await;

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].content, "User prefers SOL");
    }

    #[tokio::test]
    async fn test_long_term_memory_exact_match() {
        let path = PathBuf::from("test_memory_exact.jsonl");
        if path.exists() {
            let _ = std::fs::remove_file(&path);
        }
        let memory = LongTermMemory::new(100, path).await.unwrap();

        // 1. Store one with "SOLANA" tag
        memory.store_entry(MemoryEntry {
            id: "1".to_string(),
            user_id: "user1".to_string(),
            content: "Full Solana name".to_string(),
            timestamp: 1000,
            tags: vec!["SOLANA".to_string()],
            relevance: 1.0,
        }, None).await.unwrap();


        // 2. Store one with "SOL" tag
        memory.store_entry(MemoryEntry {
            id: "2".to_string(),
            user_id: "user1".to_string(),
            content: "Just SOL".to_string(),
            timestamp: 1001,
            tags: vec!["SOL".to_string()],
            relevance: 1.0,
        }, None).await.unwrap();


        // 3. Search for "SOL" (should only return "Just SOL")
        let entries = memory.retrieve_by_tag("user1", "SOL", None, 10).await;

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].content, "Just SOL");
    }

    #[tokio::test]
    async fn test_long_term_memory_pruning() {
        let path = PathBuf::from("test_memory_pruning.jsonl");
        if path.exists() {
            let _ = std::fs::remove_file(&path);
        }
        // Limit 10
        let memory = LongTermMemory::new(10, path.clone()).await.unwrap();

        // Insert 15 items
        for i in 0..15 {
             memory.store_entry(MemoryEntry {
                id: format!("{}", i),
                user_id: "user1".to_string(),
                content: format!("Content {}", i),
                timestamp: 1000 + i, // ascending timestamp
                tags: vec![],
                relevance: 1.0,
            }, None).await.unwrap();
        }

        // Force prune (since store_entry is probabilistic)
        memory.prune(10, "user1".to_string(), None).await;
        
        // Wait for async prune
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let all = memory.retrieve_recent("user1", None, 10000).await;
        
        assert!(all.len() <= 10, "Should have pruned to <= 10, got {}", all.len());
        
        // Oldest (0..4) should be gone, Newest (5..14) should remain
        // Check if "Content 14" is present
        assert!(all.iter().any(|e| e.content == "Content 14"));
        // Check if "Content 0" is missing
        assert!(!all.iter().any(|e| e.content == "Content 0"));

        let _ = std::fs::remove_file(&path);
    }
}
