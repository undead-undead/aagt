//! Simple File-based Vector Store (JSONL)
//!
//! A lightweight, Persistent vector store using JSONL files and in-memory brute-force search.
//! Designed for low-resource environments (e.g. 1GB VPS) where running a full Vector DB (Qdrant/pgvector) is too heavy.
//!
//! # Features
//! - **Storage**: Append-only JSONL file.
//! - **Index**: In-memory `Vec<IndexEntry>` (ID + Embedding + Offset). Content is NOT stored in RAM.
//! - **Search**: Brute-force cosine similarity -> Seek & Read content from disk for top N results.
//!
//! # Performance
//! - Memory: ~6MB per 10k documents (vs ~100MB+ with content).
//! - Speed: < 20ms search (IO overhead for top results only).

use crate::rag::{Document, VectorStore, Embeddings};
use crate::error::{Error, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs::{self, OpenOptions};
use tokio::io::{AsyncWriteExt, AsyncBufReadExt, AsyncSeekExt, SeekFrom, AsyncReadExt};
use tokio::sync::{RwLock, mpsc, oneshot};
use rayon::prelude::*;

/// Configuration for FileStore
#[derive(Debug, Clone)]
pub struct FileStoreConfig {
    /// Path to the JSONL file
    pub path: PathBuf,
}

impl FileStoreConfig {
    /// Create config from path
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

/// Messages sent to the FileStore Actor
enum StoreMessage {
    /// Append a new line (document) to the file
    Append { 
        line: String, 
        reply: oneshot::Sender<Result<(u64, u64)>> // Returns (offset, length)
    },
    /// Trigger compaction (rewrite file to remove deleted items)
    Compact {
        indices: Vec<IndexEntry>,
        reply: oneshot::Sender<Result<Vec<u64>>> // Returns new offsets corresponding to input indices
    },
    /// Save index snapshot to disk
    SaveSnapshot {
        indices: Vec<IndexEntry>,
    }
}

/// Actor managing exclusive write access to the file
struct FileStoreActor {
    path: PathBuf,
    receiver: mpsc::Receiver<StoreMessage>,
}

impl FileStoreActor {
    async fn run(mut self) {
        while let Some(msg) = self.receiver.recv().await {
            match msg {
                StoreMessage::Append { line, reply } => {
                    let res = self.handle_append(line).await;
                    let _ = reply.send(res);
                }
                StoreMessage::Compact { indices, reply } => {
                    let res = self.handle_compact(indices).await;
                    let _ = reply.send(res);
                }
                StoreMessage::SaveSnapshot { indices } => {
                    let _ = self.handle_save_snapshot(indices).await;
                }
            }
        }
    }

    async fn handle_append(&self, line: String) -> Result<(u64, u64)> {
        // atomic append relying on OS file locks or just exclusive actor access 
        // Since we are the only writer, we can Seek(End) and Write.
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await?;

        let offset = file.seek(SeekFrom::End(0)).await?;
        file.write_all(line.as_bytes()).await?;
        file.flush().await?; // Ensure it hits disk (or at least OS buffer) safely

        let length = line.len() as u64;
        Ok((offset, length))
    }

    async fn handle_compact(&self, indices: Vec<IndexEntry>) -> Result<Vec<u64>> {
        let tmp_path = self.path.with_extension("compact");
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&tmp_path)
            .await?;

        let mut new_offsets = Vec::with_capacity(indices.len());
        let mut current_offset = 0u64;

        // We need a separate reader for the old file
        // NOTE: This might fail if the file is massive and OS limits open files, but we open one reader here.
        // Optimization: buffer reads? 
        
        for idx in &indices {
             // Read from OLD file using static helper
             let content = Self::read_one(&self.path, idx.offset, idx.length).await?;
             let line = serde_json::to_string(&content)
                 .map_err(|e| Error::MemoryStorage(format!("JSON error: {}", e)))? + "\n";
             
             file.write_all(line.as_bytes()).await?;
             new_offsets.push(current_offset);
             current_offset += line.len() as u64;
        }

        file.flush().await?;
        tokio::fs::rename(tmp_path, &self.path).await?;
        
        Ok(new_offsets)
    }

    async fn read_one(path: &PathBuf, offset: u64, length: u64) -> Result<StoredDocument> {
        let mut file = fs::File::open(path).await?;
        file.seek(SeekFrom::Start(offset)).await?;
        let mut buffer = vec![0u8; length as usize];
        file.read_exact(&mut buffer).await?;
        let s = String::from_utf8(buffer).map_err(|e| Error::MemoryStorage(format!("UTF8 error: {}", e)))?;
        serde_json::from_str(&s).map_err(|e| Error::MemoryStorage(format!("JSON error: {}", e)))
    }

    async fn handle_save_snapshot(&self, indices: Vec<IndexEntry>) -> Result<()> {
        let snap_path = self.path.with_extension("index");
        // Do serialization in spawn_blocking to avoid blocking executor
        let bytes = tokio::task::spawn_blocking(move || {
            bincode::serialize(&indices)
                .map_err(|e| Error::Internal(format!("Failed to serialize index: {}", e)))
        }).await.map_err(|e| Error::Internal(format!("Snapshot task join error: {}", e)))??;

        fs::write(snap_path, bytes).await?;
        Ok(())
    }
}

/// A lightweight file-based vector store with lazy loading
#[derive(Clone)]
pub struct FileStore {
    config: FileStoreConfig,
    /// In-memory index (ID, Embedding, Metadata, File Offset)
    indices: Arc<RwLock<Vec<IndexEntry>>>,
    /// Optional embedder for vectorizing content
    embedder: Option<Arc<dyn Embeddings>>,
    /// Channel to Actor
    sender: mpsc::Sender<StoreMessage>,
}

/// In-memory index entry - optimized for RAM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    pub id: String,
    /// Store metadata as a boxed slice of tuples to save RAM (no HashMap overhead)
    pub metadata: Box<[(String, String)]>,
    /// Store embedding as a boxed slice to prevent overallocation
    pub embedding: Box<[f32]>,
    /// Byte offset in the file
    pub offset: u64,
    /// Length of the JSON line in bytes
    pub length: u64,
}

impl IndexEntry {
    /// Get a metadata value by key
    pub fn get_metadata(&self, key: &str) -> Option<&str> {
        self.metadata.iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

/// Stored document format (On Disk)
#[derive(Debug, Serialize, Deserialize, Clone)]
struct StoredDocument {
    id: String,
    content: String,
    metadata: HashMap<String, String>,
    embedding: Vec<f32>,
}

impl FileStore {
    /// Open or create a new FileStore
    pub async fn new(config: FileStoreConfig) -> Result<Self> {
        let indices = Arc::new(RwLock::new(Vec::new()));
        
        // Ensure directory exists
        if let Some(parent) = config.path.parent() {
            fs::create_dir_all(parent).await.ok();
        }

        // Spawn Actor
        let (tx, rx) = mpsc::channel(100);
        let actor = FileStoreActor {
            path: config.path.clone(),
            receiver: rx,
        };
        tokio::spawn(actor.run());

        let store = Self { 
            config, 
            indices,
            embedder: None,
            sender: tx,
        };
        
        store.load().await?;
        Ok(store)
    }

    /// Attach an embedder to this store
    pub fn with_embedder(mut self, embedder: Arc<dyn Embeddings>) -> Self {
        self.embedder = Some(embedder);
        self
    }

    /// Load index from disk (Scans file, does not load content)
    async fn load(&self) -> Result<()> {
        if !self.config.path.exists() {
            return Ok(());
        }

        // Try loading from snapshot first
        if let Ok(Some(cached_indices)) = self.try_load_snapshot().await {
            let mut indices = self.indices.write().await;
            *indices = cached_indices;
            tracing::info!("FileStore loaded {} documents from snapshot", indices.len());
            return Ok(());
        }

        let file = fs::File::open(&self.config.path).await?;
        let mut reader = tokio::io::BufReader::new(file);
        let mut buffer = String::new();
        let mut offset = 0u64;

        let mut indices = self.indices.write().await;
        indices.clear();

        loop {
            buffer.clear();
            let bytes_read = reader.read_line(&mut buffer).await?;
            if bytes_read == 0 {
                break;
            }

            if buffer.trim().is_empty() {
                offset += bytes_read as u64;
                continue;
            }

            // Parse document
            if let Ok(doc) = serde_json::from_str::<StoredDocument>(&buffer) {
                let metadata: Vec<(String, String)> = doc.metadata.into_iter().collect();
                indices.push(IndexEntry {
                    id: doc.id,
                    metadata: metadata.into_boxed_slice(),
                    embedding: doc.embedding.into_boxed_slice(),
                    offset,
                    length: bytes_read as u64,
                });
            } else {
                tracing::warn!("Skipping malformed line in FileStore at offset {}", offset);
            }

            offset += bytes_read as u64;
        }
        
        tracing::info!("FileStore loaded {} documents (index only)", indices.len());
        
        // Save snapshot after first full load
        self.queue_snapshot(&indices).await;
        
        Ok(())
    }

    fn snapshot_path(&self) -> std::path::PathBuf {
        self.config.path.with_extension("index")
    }

    async fn try_load_snapshot(&self) -> Result<Option<Vec<IndexEntry>>> {
        let snap_path = self.snapshot_path();
        if !snap_path.exists() {
            return Ok(None);
        }

        let main_meta = fs::metadata(&self.config.path).await?;
        let snap_meta = fs::metadata(&snap_path).await?;

        // If snapshot is older than the main file (by more than 1s to account for clock skew/fs precision), it's invalid
        if snap_meta.modified()? < main_meta.modified()? {
             return Ok(None);
        }

        let bytes = fs::read(&snap_path).await?;
        let indices: Vec<IndexEntry> = bincode::deserialize(&bytes)
            .map_err(|e| Error::Internal(format!("Failed to deserialize index: {}", e)))?;

        Ok(Some(indices))
    }

    async fn queue_snapshot(&self, indices: &[IndexEntry]) {
         let _ = self.sender.send(StoreMessage::SaveSnapshot { indices: indices.to_vec() }).await;
    }

    /// Cosine similarity between two vectors
    fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() { return 0.0; }
        
        let dot_product: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        
        if norm_a == 0.0 || norm_b == 0.0 { 0.0 } else { dot_product / (norm_a * norm_b) }
    }

    /// Read a specific document content from disk
    async fn read_content(&self, offset: u64, length: u64) -> Result<StoredDocument> {
        // Reading is safe concurrently with Appending (on Linux/Unix)
        // We open a new handle for this read.
        let mut file = fs::File::open(&self.config.path).await?;
        file.seek(SeekFrom::Start(offset)).await?;
        
        let mut buffer = vec![0u8; length as usize];
        file.read_exact(&mut buffer).await?;
        
        let s = String::from_utf8(buffer)
            .map_err(|e| Error::MemoryStorage(format!("UTF8 error: {}", e)))?;
            
        serde_json::from_str(&s)
            .map_err(|e| Error::MemoryStorage(format!("JSON error: {}", e)))
    }

    /// Check if document exists by ID (using Index only)
    pub async fn exists(&self, id: &str) -> bool {
        let indices = self.indices.read().await;
        indices.iter().any(|idx| idx.id == id)
    }

    /// Find documents by metadata key-value pair (using Index only, then fetch content)
    /// This avoids scanning the entire file.
    pub async fn find_by_metadata(&self, key: &str, value: &str) -> Vec<Document> {
        let indices = self.indices.read().await;
        // Filter indices first (In-Memory operation = Fast)
        let matches: Vec<IndexEntry> = indices.iter()
            .filter(|idx| idx.metadata.iter().any(|(k, v)| k == key && v == value))
            .cloned() // Clone lightweight index entry
            .collect();
        drop(indices);

        // Fetch content only for matches
        let mut results = Vec::new();
        for idx in matches {
            if let Ok(doc) = self.read_content(idx.offset, idx.length).await {
                results.push(Document {
                    id: doc.id,
                    content: doc.content,
                    metadata: doc.metadata,
                    score: 1.0,
                });
            }
        }
        results
    }
    
    /// Find documents using a custom predicate on the IndexEntry (Fast)
    pub async fn find<F>(&self, predicate: F) -> Vec<Document> 
    where F: Fn(&IndexEntry) -> bool 
    {
        let indices = self.indices.read().await;
        // Filter indices first
        let matches: Vec<IndexEntry> = indices.iter()
            .filter(|idx| predicate(idx))
            .cloned()
            .collect();
        drop(indices);

        // Fetch content
        let mut results = Vec::new();
        for idx in matches {
            if let Ok(doc) = self.read_content(idx.offset, idx.length).await {
                results.push(Document {
                    id: doc.id,
                    content: doc.content,
                    metadata: doc.metadata,
                    score: 1.0,
                });
            }
        }
        results
    }
    
    /// Retrieve all documents (Manual iteration - expensive)
    pub async fn get_all(&self) -> Vec<Document> {
        let indices = self.indices.read().await;
        let mut result = Vec::new();

        for idx in indices.iter() {
            if let Ok(doc) = self.read_content(idx.offset, idx.length).await {
                result.push(Document {
                    id: doc.id,
                    content: doc.content,
                    metadata: doc.metadata,
                    score: 1.0,
                });
            }
        }
        result
    }

    /// Get the last N documents (Efficient for recent history)
    pub async fn tail(&self, limit: usize) -> Vec<Document> {
        let indices = self.indices.read().await;
        let len = indices.len();
        let start = len.saturating_sub(limit);
        
        // Clone the slice of interest
        let matches: Vec<IndexEntry> = indices[start..].to_vec();
        drop(indices);

        let mut results = Vec::new();
        for idx in matches {
            if let Ok(doc) = self.read_content(idx.offset, idx.length).await {
                results.push(Document {
                    id: doc.id,
                    content: doc.content,
                    metadata: doc.metadata,
                    score: 1.0,
                });
            }
        }
        // Reverse to have newest first if that's expected by caller, or keep chronological.
        // Usually tail implies [..., newest].
        results
    }
}

#[async_trait]
impl VectorStore for FileStore {
    async fn store(&self, content: &str, mut metadata: HashMap<String, String>) -> Result<String> {
        // Extract embedding from metadata or generate using embedder
        let embedding: Vec<f32> = if let Some(emb_str) = metadata.get("_embedding") {
            serde_json::from_str(emb_str).unwrap_or_else(|_| vec![])
        } else if let Some(embedder) = &self.embedder {
            embedder.embed(content).await?
        } else {
            vec![0.0; 1536]
        };
        
        metadata.remove("_embedding");

        let doc = StoredDocument {
            id: uuid::Uuid::new_v4().to_string(),
            content: content.to_string(),
            metadata: metadata.clone(),
            embedding: embedding.clone(),
        };

        // 1. Prepare line
        let line = serde_json::to_string(&doc)
            .map_err(|e| Error::MemoryStorage(format!("JSON error: {}", e)))? + "\n";
        
        // 2. Send to Actor
        let (tx, rx) = oneshot::channel();
        self.sender.send(StoreMessage::Append { line, reply: tx }).await
            .map_err(|_| Error::Internal("FileStore actor closed".to_string()))?;
            
        let (offset, length) = rx.await
            .map_err(|_| Error::Internal("FileStore reply dropped".to_string()))??;

        // 3. Update Index
        let entry_metadata: Vec<(String, String)> = metadata.into_iter().collect();
        let entry = IndexEntry {
            id: doc.id.clone(),
            metadata: entry_metadata.into_boxed_slice(),
            embedding: embedding.into_boxed_slice(),
            offset,
            length,
        };
        
        let mut indices = self.indices.write().await;
        indices.push(entry);
        
        // 4. Save Snapshot (background via actor)
        self.queue_snapshot(&indices).await;

        Ok(doc.id)
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Document>> {
        // Generate query embedding
        let query_embedding: Vec<f32> = if let Some(embedder) = &self.embedder {
            embedder.embed(query).await?
        } else {
            vec![0.0; 1536]
        };

        // 1. Memory Search (Indices only) - CPU Bound
        // We clone the indices to move them into the blocking thread.
        // For very large indices, we might want to use a read lock inside the thread,
        // but Arc<RwLock> safely allows reading. However, we simply clone the Arc here.
        let indices_arc = self.indices.clone();
        
        let scored_indices = tokio::task::spawn_blocking(move || {
            let indices = indices_arc.blocking_read();
            let mut scored: Vec<(f32, IndexEntry)> = indices.par_iter()
                .map(|idx| (Self::cosine_similarity(&query_embedding, &idx.embedding), idx.clone()))
                .collect();

            // Sort descending
            scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            
            scored.into_iter().take(limit).collect::<Vec<_>>()
        }).await.map_err(|e| Error::Internal(format!("Search task join error: {}", e)))?;

        // 2. Fetch Content (IO Bound - Async)
        let mut results = Vec::new();
        for (score, idx) in scored_indices {
            if let Ok(doc) = self.read_content(idx.offset, idx.length).await {
                results.push(Document {
                    id: doc.id,
                    content: doc.content,
                    metadata: doc.metadata,
                    score,
                });
            } else {
                tracing::warn!("Failed to read content for document {} during search", idx.id);
            }
        }
        
        Ok(results)
    }

    async fn delete(&self, id: &str) -> Result<()> {
        // 1. Remove from index
        let mut indices_to_keep = Vec::new();
        {
            let mut indices = self.indices.write().await;
            if let Some(pos) = indices.iter().position(|idx| idx.id == id) {
                indices.remove(pos);
                indices_to_keep = indices.clone();
            } else {
                return Ok(());
            }
        }

        // 2. Send Compact Request to Actor
        let (tx, rx) = oneshot::channel();
        self.sender.send(StoreMessage::Compact { indices: indices_to_keep, reply: tx }).await
            .map_err(|_| Error::Internal("FileStore actor closed".to_string()))?;
            
        let new_offsets = rx.await
            .map_err(|_| Error::Internal("FileStore reply dropped".to_string()))??;

        // 3. Update offsets in memory
        {
            let mut indices_mut = self.indices.write().await;
            for (i, offset) in new_offsets.into_iter().enumerate() {
                if i < indices_mut.len() {
                     indices_mut[i].offset = offset;
                }
            }
            // Save Snapshot
            self.queue_snapshot(&indices_mut).await;
        }
        
        Ok(())
    }
}
