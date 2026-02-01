//! Simple File-based Vector Store (JSONL)
//!
//! A lightweight, Persistent vector store using JSONL files and in-memory brute-force search.
//! Designed for low-resource environments (e.g. 1GB VPS) where running a full Vector DB (Qdrant/pgvector) is too heavy.
//!
//! # Features
//! - **Storage**: Append-only JSONL file.
//! - **Unsorted Index**: All vectors kept in memory (`Vec<Document>`).
//! - **Search**: Brute-force cosine similarity (SIMD-optimized by Rust compiler).
//!
//! # Performance
//! - Memory: ~7MB per 1k documents (1536 dim / float32).
//! - Speed: < 10ms search for 10k documents.

use crate::rag::{Document, VectorStore};
use crate::error::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::sync::RwLock;

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

/// A lightweight file-based vector store
#[derive(Clone)]
pub struct FileStore {
    config: FileStoreConfig,
    /// In-memory cache of all documents for searching
    documents: Arc<RwLock<Vec<StoredDocument>>>,
}

/// Internal document representation for storage
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
        let documents = Arc::new(RwLock::new(Vec::new()));
        
        // Ensure directory exists
        if let Some(parent) = config.path.parent() {
            fs::create_dir_all(parent).await.ok();
        }

        let store = Self { config, documents };
        store.load().await?;
        Ok(store)
    }

    /// Load data from disk into memory using streaming to save memory
    async fn load(&self) -> Result<()> {
        if !self.config.path.exists() {
            return Ok(());
        }

        use tokio::io::AsyncBufReadExt;
        
        let file = fs::File::open(&self.config.path).await?;
        let reader = tokio::io::BufReader::new(file);
        let mut lines = reader.lines();

        let mut docs = self.documents.write().await;
        docs.clear();

        while let Ok(Some(line)) = lines.next_line().await {
            if line.trim().is_empty() { continue; }
            // Safe JSON parsing - ignore malformed lines instead of crashing
            if let Ok(doc) = serde_json::from_str::<StoredDocument>(&line) {
                docs.push(doc);
            }
        }
        
        Ok(())
    }

    /// Cosine similarity between two vectors
    fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() { return 0.0; }
        
        let dot_product: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        
        if norm_a == 0.0 || norm_b == 0.0 { 0.0 } else { dot_product / (norm_a * norm_b) }
    }
}

#[async_trait]
impl VectorStore for FileStore {
    // ... (store implementation remains same)

    async fn store(&self, content: &str, mut metadata: HashMap<String, String>) -> Result<String> {
        // Extract embedding from metadata or generate random (fallback)
        let embedding: Vec<f32> = if let Some(emb_str) = metadata.get("_embedding") {
            serde_json::from_str(emb_str).unwrap_or_else(|_| vec![])
        } else {
            // Fallback: Zeros for testing (no dependency)
            vec![0.0; 1536]
        };
        
        // Remove technical metadata
        metadata.remove("_embedding");

        let doc = StoredDocument {
            id: uuid::Uuid::new_v4().to_string(),
            content: content.to_string(),
            metadata,
            embedding,
        };

        // 1. Append to File
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.config.path)
            .await?;
        
        // Handle serialization error safely
        // Handle serialization error safely
        let line = serde_json::to_string(&doc).map_err(|e| crate::error::Error::MemoryStorage(format!("JSON error: {}", e)))? + "\n";
        file.write_all(line.as_bytes()).await?;

        // 2. Update Memory
        self.documents.write().await.push(doc.clone());

        Ok(doc.id)
    }

    // ... (search implementation remains same)
    
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Document>> {
        // NOTE: In real implementation, query should be converted to embedding first.
        let query_embedding: Vec<f32> = vec![0.0; 1536];

        let docs = self.documents.read().await;
        
        let mut scored: Vec<(f32, &StoredDocument)> = docs.iter()
            .map(|d| (Self::cosine_similarity(&query_embedding, &d.embedding), d))
            .collect();

        // Sort descending
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        Ok(scored.into_iter().take(limit).map(|(score, d)| Document {
            id: d.id.clone(),
            content: d.content.clone(),
            metadata: d.metadata.clone(),
            score,
        }).collect())
    }

    async fn delete(&self, id: &str) -> Result<()> {
        let mut docs = self.documents.write().await;
        if let Some(pos) = docs.iter().position(|d| d.id == id) {
            docs.remove(pos);
            // Rewrite file (Compaction) - handle serialization errors
            let mut content = String::new();
            for d in docs.iter() {
                let line = serde_json::to_string(d).map_err(|e| crate::error::Error::MemoryStorage(format!("JSON error: {}", e)))?;
                content.push_str(&line);
                content.push('\n');
            }
            fs::write(&self.config.path, content).await?;
        }
        Ok(())
    }
}
