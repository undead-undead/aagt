/// Simple File-based Vector Store
/// 
/// A lightweight, persistent vector store that uses JSONL files.
/// Ideally suited for "poor man's VPS" (low memory, no external database).
/// 
/// # Architecture
/// - **Storage**: Append-only JSONL file (`memory.jsonl`)
/// - **Index**: In-memory dense vector scan (brute-force)
/// - **Performance**: Fast enough for < 50k items
/// - **Memory Usage**: ~60MB for 10k items (OpenAI embeddings)

use aagt_core::rag::{Document, VectorStore};
use aagt_core::error::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct SimpleFileStore {
    path: PathBuf,
    // In-memory cache for fast search
    documents: Arc<RwLock<Vec<StoredDocument>>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct StoredDocument {
    id: String,
    content: String,
    metadata: HashMap<String, String>,
    embedding: Vec<f32>,
}

impl SimpleFileStore {
    pub async fn new(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let documents = Arc::new(RwLock::new(Vec::new()));
        
        // Load existing data if file exists
        if path.exists() {
            let content = fs::read_to_string(&path).await.unwrap_or_default();
            let mut docs = documents.write().await;
            for line in content.lines() {
                if !line.trim().is_empty() {
                    if let Ok(doc) = serde_json::from_str::<StoredDocument>(line) {
                        docs.push(doc);
                    }
                }
            }
        }

        Ok(Self { path, documents })
    }

    /// Calculate cosine similarity
    fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        let dot_product: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm_a == 0.0 || norm_b == 0.0 { 0.0 } else { dot_product / (norm_a * norm_b) }
    }
}

#[async_trait]
impl VectorStore for SimpleFileStore {
    async fn store(&self, content: &str, metadata: HashMap<String, String>) -> Result<String> {
        // NOTE: In a real app, you need to generate embedding here.
        // For this example, we simulate a random embedding.
        // In Listen, call your Embedding API provider here.
        let embedding: Vec<f32> = (0..1536).map(|_| rand::random::<f32>()).collect(); 
        
        let doc = StoredDocument {
            id: uuid::Uuid::new_v4().to_string(),
            content: content.to_string(),
            metadata,
            embedding,
        };

        // 1. Write to file (Append)
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await?;
        
        let line = serde_json::to_string(&doc)? + "\n";
        file.write_all(line.as_bytes()).await?;

        // 2. Update memory
        self.documents.write().await.push(doc.clone());

        Ok(doc.id)
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Document>> {
        // NOTE: Simulate query embedding
        let query_embedding: Vec<f32> = (0..1536).map(|_| rand::random::<f32>()).collect();

        let docs = self.documents.read().await;
        
        // Brute-force cosine similarity scan
        let mut scored_docs: Vec<(f32, &StoredDocument)> = docs.iter()
            .map(|doc| {
                let score = Self::cosine_similarity(&query_embedding, &doc.embedding);
                (score, doc)
            })
            .collect();

        // Sort by score descending
        scored_docs.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        // Return top K
        Ok(scored_docs.into_iter()
            .take(limit)
            .map(|(score, doc)| Document {
                id: doc.id.clone(),
                content: doc.content.clone(),
                metadata: doc.metadata.clone(),
                score,
            })
            .collect())
    }

    async fn delete(&self, _id: &str) -> Result<()> {
        // Delete in JSONL is hard (rewrite file). 
        // For "Poor Man's Store", we mostly append.
        // todo: Implement rewrite logic
        Ok(())
    }
}
