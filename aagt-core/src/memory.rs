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
            if entry.len() >= self.max_messages {
                entry.pop_front();
            }
            entry.push_back(message);
        } // Lock on DashMap bucket dropped here
        
        // Update access time
        self.last_access.insert(key, std::time::Instant::now());
        
        // Save immediately for safety (Async I/O)
        // Now safe because we are no longer holding the DashMap write lock
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

    async fn clear(&self, user_id: &str, agent_id: Option<&str>) -> crate::error::Result<()> {
        let key = self.key(user_id, agent_id);
        self.store.remove(&key);
        self.last_access.remove(&key);
        
        // Sync to disk
        self.save().await
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

/// Filter for tags during retrieval
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TagFilter {
    /// Include only if it has ANY of these tags
    IncludeAny(Vec<String>),
    /// Exclude if it has ANY of these tags
    ExcludeAny(Vec<String>),
    /// No filtering
    None,
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
        // Serialize tags - fail loudly to maintain data integrity
        let tags_json = serde_json::to_string(&entry.tags)
            .map_err(|e| crate::error::Error::Internal(format!("Failed to serialize tags: {}", e)))?;
        metadata.insert("tags".to_string(), tags_json);
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
    /// Fix H1: Synchronous execution with proper error handling
    pub async fn prune(&self, limit: usize, user_id: String, agent_id: Option<String>) -> Result<(), crate::error::Error> {
        let uid = user_id.clone();
        let aid = agent_id.clone();
        
        // Find all IDs for this user/agent
        let docs = self.store.find_metadata(move |idx| {
            if idx.get_metadata("user_id") != Some(&uid) { return false; }
            if let Some(ref target_aid) = aid {
                if idx.get_metadata("agent_id") != Some(target_aid) { return false; }
            }
            true
        }).await;

        if docs.len() <= limit {
            return Ok(());
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
            // Collect IDs to delete
            let ids_to_delete: Vec<String> = entries.into_iter()
                .take(to_remove)
                .map(|(id, _)| id)
                .collect();

            tracing::info!("Pruning {} old entries for user {} (agent {:?})", 
                ids_to_delete.len(), user_id, agent_id);

            // Batch Delete (Single Snapshot Save)
            self.store.delete_batch(ids_to_delete).await?;
            
            // Trigger compaction if we deleted a lot
            if to_remove > 100 {
                self.store.auto_compact(limit).await?;
            }
        }
        
        Ok(())
    }

    /// Retrieve entries by tag with optional agent isolation
    pub async fn retrieve_by_tag(&self, user_id: &str, tag: &str, agent_id: Option<&str>, limit: usize) -> Vec<MemoryEntry> {
        self.retrieve_filtered(user_id, agent_id, TagFilter::IncludeAny(vec![tag.to_string()]), limit).await
    }

    /// Retrieve recent entries with token awareness (approximate) and optional agent isolation
    pub async fn retrieve_recent(&self, user_id: &str, agent_id: Option<&str>, char_limit: usize) -> Vec<MemoryEntry> {
        // Default behavior: Exclude "archived" and "do_not_rag"
        let filter = TagFilter::ExcludeAny(vec!["archived".to_string(), "do_not_rag".to_string()]);
        
        let uid = user_id.to_string();
        let aid = agent_id.map(|s| s.to_string());
        
        let matches = self.store.find_metadata(move |idx| {
            let matches_user = idx.get_metadata("user_id") == Some(&uid);
            let matches_agent = aid.is_none() || idx.get_metadata("agent_id") == aid.as_deref();
            
            if !matches_user || !matches_agent {
                return false;
            }

            // Apply Tag Filter
            if let Some(tags_json) = idx.get_metadata("tags") {
                let tags: Vec<String> = serde_json::from_str(tags_json).unwrap_or_default();
                match &filter {
                    TagFilter::IncludeAny(include_tags) => {
                        include_tags.iter().any(|t| tags.contains(t))
                    }
                    TagFilter::ExcludeAny(exclude_tags) => {
                        !exclude_tags.iter().any(|t| tags.contains(t))
                    }
                    TagFilter::None => true,
                }
            } else {
                // If no tags, include unless we require inclusion
                match &filter {
                    TagFilter::IncludeAny(_) => false,
                    _ => true,
                }
            }
        }).await;

        if matches.is_empty() {
            return Vec::new();
        }

        // Parse timestamps and sort by timestamp descending (newest first)
        let mut sorted_indices: Vec<(crate::store::file::IndexEntry, i64)> = matches.into_iter().filter_map(|idx| {
            let ts = idx.get_metadata("timestamp")?.parse::<i64>().ok()?;
            Some((idx, ts))
        }).collect();
        sorted_indices.sort_by(|a, b| b.1.cmp(&a.1));

        let mut result_entries = Vec::new();
        let mut current_chars = 0;

        // Hydrate entries one by one until char_limit is reached
        for (idx, _) in sorted_indices {
            // We need to hydrate to get the content length
            if let Some(doc) = self.store.get(&idx.id).await.ok().flatten() {
                if current_chars + doc.content.len() > char_limit {
                    break;
                }
                if let Some(entry) = Self::doc_to_entry(doc) {
                    current_chars += entry.content.len();
                    result_entries.push(entry);
                }
            }
        }
        result_entries
    }
    
    /// Advanced retrieval with explicit tag filtering
    pub async fn retrieve_filtered(&self, user_id: &str, agent_id: Option<&str>, filter: TagFilter, limit: usize) -> Vec<MemoryEntry> {
        let uid = user_id.to_string();
        let aid = agent_id.map(|s| s.to_string());
        let filter_clone = filter.clone();

        let docs = self.store.find(move |idx| {
            if idx.get_metadata("user_id") != Some(&uid) { return false; }
            if let Some(ref target_aid) = aid {
                if idx.get_metadata("agent_id") != Some(target_aid) { return false; }
            }
            
            if let Some(tags_json) = idx.get_metadata("tags") {
                let tags: Vec<String> = serde_json::from_str(tags_json).unwrap_or_default();
                match &filter_clone {
                    TagFilter::IncludeAny(include_tags) => {
                        include_tags.iter().any(|t| tags.contains(t))
                    }
                    TagFilter::ExcludeAny(exclude_tags) => {
                        !exclude_tags.iter().any(|t| tags.contains(t))
                    }
                    TagFilter::None => true,
                }
            } else {
                match &filter_clone {
                    TagFilter::IncludeAny(_) => false,
                    _ => true,
                }
            }
        }).await;
        
        docs.into_iter()
            .filter_map(|doc| Self::doc_to_entry(doc))
            .take(limit)
            .collect()
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
        
        // Fix #1: Use delete_batch to avoid O(N^2) work (N snapshots for N items)
        let ids_to_delete = store.find_ids(move |idx| {
            if idx.get_metadata("user_id") != Some(&uid) { return false; }
            if let Some(ref target_aid) = aid {
                 if idx.get_metadata("agent_id") != Some(target_aid) { return false; }
            }
            true
        }).await;

        if !ids_to_delete.is_empty() {
            tracing::info!("Clearing memory for user {}{}: deleting {} entries in batch", user_id, agent_id.map_or("".to_string(), |a| format!(" (agent {})", a)), ids_to_delete.len());
            store.delete_batch(ids_to_delete).await?;
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
        let short_term_path = path.with_extension("stm.json");
        Ok(Self {
            short_term: Arc::new(ShortTermMemory::new(short_term_max, short_term_users, short_term_path).await),
            long_term: Arc::new(LongTermMemory::new(long_term_max, path).await?),
        })
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

    #[tokio::test]
    async fn test_short_term_persistence() {
        let path = PathBuf::from("test_stm_persist.json");
        // Clean start
        if path.exists() { let _ = std::fs::remove_file(&path); }
        
        {
            let memory = ShortTermMemory::new(100, 10, path.clone()).await;
            memory.store("user1", None, Message::user("Memory check")).await.unwrap();
            // Should save automatically
        }
        
        // Relief from disk
        let memory2 = ShortTermMemory::new(100, 10, path.clone()).await;
        let msgs = memory2.retrieve("user1", None, 10).await;
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].text(), "Memory check");
        
        let _ = std::fs::remove_file(&path);
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
        let _ = memory.prune(10, "user1".to_string(), None).await;
        
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
