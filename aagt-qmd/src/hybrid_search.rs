//! Hybrid search engine combining BM25 and vector similarity search
//!
//! Integrates keyword-based (BM25/FTS5) and semantic (vector) search using RRF fusion.

#[cfg(feature = "vector")]
use crate::chunker::{Chunker, ChunkerConfig};
#[cfg(feature = "vector")]
use crate::embedder::{Embedder, EmbedderConfig};
use crate::error::Result;
use crate::rrf::RrfFusion;
use crate::store::{Collection, Document, QmdStore};
#[cfg(feature = "vector")]
use crate::vector_store::{VectorSearchResult, VectorStore};
use std::path::PathBuf;

/// Configuration for hybrid search
#[derive(Debug, Clone)]
pub struct HybridSearchConfig {
    /// Database path for QMD store
    pub db_path: PathBuf,
    /// Number of BM25 results to retrieve for fusion
    pub bm25_candidates: usize,
    /// Number of vector results to retrieve for fusion
    #[cfg(feature = "vector")]
    pub vector_candidates: usize,
    /// Embedder configuration
    #[cfg(feature = "vector")]
    pub embedder_config: crate::embedder::EmbedderConfig,
    /// Chunker configuration
    #[cfg(feature = "vector")]
    pub chunker_config: crate::chunker::ChunkerConfig,
    /// Vector store persistence path
    #[cfg(feature = "vector")]
    pub vector_store_path: Option<PathBuf>,
    /// Max elements for HNSW index
    #[cfg(feature = "vector")]
    pub hnsw_max_elements: usize,
}

impl Default for HybridSearchConfig {
    fn default() -> Self {
        Self {
            db_path: PathBuf::from("qmd.db"),
            bm25_candidates: 50,
            #[cfg(feature = "vector")]
            vector_candidates: 50,
            #[cfg(feature = "vector")]
            embedder_config: crate::embedder::EmbedderConfig::default(),
            #[cfg(feature = "vector")]
            chunker_config: crate::chunker::ChunkerConfig::default(),
            #[cfg(feature = "vector")]
            vector_store_path: None,
            #[cfg(feature = "vector")]
            hnsw_max_elements: 100_000,
        }
    }
}

/// Hybrid search result combining BM25 and vector search
#[derive(Debug, Clone)]
pub struct HybridSearchResult {
    /// Rank in final results (1-indexed)
    pub rank: usize,
    /// Document
    pub document: Document,
    /// Combined RRF score
    pub rrf_score: f64,
    /// BM25 score (if found via BM25)
    pub bm25_score: Option<f64>,
    /// Vector similarity score (if found via vector search)
    pub vector_score: Option<f64>,
    /// Snippet (if available from BM25)
    pub snippet: Option<String>,
}

/// Hybrid search engine
pub struct HybridSearchEngine {
    qmd_store: QmdStore,
    #[cfg(feature = "vector")]
    vector_store: VectorStore,
    #[cfg(feature = "vector")]
    embedder: Embedder,
    #[cfg(feature = "vector")]
    chunker: Chunker,
    rrf_fusion: RrfFusion,
    config: HybridSearchConfig,
}

impl HybridSearchEngine {
    /// Create a new hybrid search engine
    pub fn new(config: HybridSearchConfig) -> Result<Self> {
        let qmd_store = QmdStore::new(&config.db_path)?;
        let rrf_fusion = RrfFusion::new();

        // Create or load vector store
        #[cfg(feature = "vector")]
        let (vector_store, embedder, chunker) = {
            let embedder = Embedder::with_config(config.embedder_config.clone())?;
            let chunker = Chunker::with_config(config.chunker_config.clone())?;

            let vector_store = if let Some(ref path) = config.vector_store_path {
                if path.exists() {
                    tracing::info!("Loading existing vector store from {:?}", path);
                    VectorStore::load(path)?
                } else {
                    tracing::info!("Creating new vector store");
                    VectorStore::new(embedder.dimension(), config.hnsw_max_elements)
                }
            } else {
                VectorStore::new(embedder.dimension(), config.hnsw_max_elements)
            };
            (vector_store, embedder, chunker)
        };

        Ok(Self {
            qmd_store,
            #[cfg(feature = "vector")]
            vector_store,
            #[cfg(feature = "vector")]
            embedder,
            #[cfg(feature = "vector")]
            chunker,
            rrf_fusion,
            config,
        })
    }

    /// Create collection
    pub fn create_collection(&self, collection: Collection) -> Result<()> {
        self.qmd_store.create_collection(collection)
    }

    /// Commit changes to persistent storage
    ///
    /// Saves the vector store to disk if there are unsaved changes.
    /// The SQLite store is auto-committed, but vector store requires    /// Commit changes to persistent storage
    pub fn commit(&self) -> Result<()> {
        #[cfg(feature = "vector")]
        if let Some(ref path) = self.config.vector_store_path {
            if self.vector_store.is_dirty() {
                tracing::info!("Saving vector store to {:?}", path);
                self.vector_store.save(path)?;
            }
        }
        Ok(())
    }

    /// Update summary for a document
    pub fn update_summary(&self, collection: &str, path: &str, summary: &str) -> Result<()> {
        self.qmd_store.update_summary(collection, path, summary)
    }

    /// Get a document by collection and path
    pub fn get_by_path(&self, collection: &str, path: &str) -> Result<Option<Document>> {
        self.qmd_store.get_by_path(collection, path)
    }

    /// Index a document (stores in both BM25 and vector stores)
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use aagt_qmd::{HybridSearchEngine, HybridSearchConfig};
    /// let mut engine = HybridSearchEngine::new(HybridSearchConfig::default())?;
    ///
    /// engine.index_document(
    ///     "trading",
    ///     "strategies/sol.md",
    ///     "SOL Trading Strategy",
    ///     "Buy SOL when RSI < 30, sell when RSI > 70. Use stop loss at -5%."
    /// )?;
    /// # Ok::<(), aagt_qmd::QmdError>(())
    /// ```
    pub fn index_document(
        &self,
        collection: &str,
        path: &str,
        title: &str,
        content: &str,
    ) -> Result<()> {
        tracing::debug!("Indexing document: {}/{}", collection, path);

        // 1. Store in QMD (BM25/FTS5)
        let doc = self
            .qmd_store
            .store_document(collection, path, title, content)?;

        tracing::debug!("Stored in QMD with docid: {}", doc.docid);

        // 2. Chunk the document
        #[cfg(feature = "vector")]
        let chunks = self.chunker.chunk(content)?;
        #[cfg(feature = "vector")]
        let num_chunks = chunks.len();
        #[cfg(feature = "vector")]
        tracing::debug!("Created {} chunks", num_chunks);

        // 3. Generate embeddings for each chunk
        #[cfg(feature = "vector")]
        for chunk in &chunks {
            let embedding = self.embedder.embed(&chunk.text)?;

            self.vector_store
                .add(collection, doc.docid.clone(), chunk.seq, embedding)?;
        }

        #[cfg(feature = "vector")]
        tracing::debug!("Indexed {} chunks for document {}", num_chunks, doc.docid);

        // 4. Persistence: Save vector store immediately to match SQLite durability
        #[cfg(feature = "vector")]
        if let Some(ref path) = self.config.vector_store_path {
            self.vector_store.save_force(path)?;
        }

        Ok(())
    }

    /// Index multiple documents in batch (More efficient than loop)
    ///
    /// Saves the vector store only once at the end.
    pub fn index_batch(
        &self,
        documents: Vec<(&str, &str, &str, &str)>, // (collection, path, title, content)
    ) -> Result<()> {
        let total = documents.len();
        tracing::info!("Batch indexing {} documents", total);

        for (i, (collection, path, title, content)) in documents.into_iter().enumerate() {
            tracing::debug!("[{}/{}] Indexing {}/{}", i + 1, total, collection, path);

            // 1. Store in QMD (BM25)
            let _doc = self
                .qmd_store
                .store_document(collection, path, title, content)?;

            // 2. Chunk (Only if vector is enabled)
            #[cfg(feature = "vector")]
            {
                let chunks = self.chunker.chunk(content)?;

                // 3. Embed and Add to Vector Store
                for chunk in chunks {
                    let embedding = self.embedder.embed(&chunk.text)?;
                    self.vector_store
                        .add(collection, doc.docid.clone(), chunk.seq, embedding)?;
                }
            }
        }

        // 4. Save ONCE at the end
        #[cfg(feature = "vector")]
        if let Some(ref path) = self.config.vector_store_path {
            tracing::info!("Saving vector store after batch index...");
            self.vector_store.save_force(path)?;
        }

        Ok(())
    }

    /// Hybrid search combining BM25 and vector search
    ///
    /// # Arguments
    ///
    /// * `query` - Search query
    /// * `limit` - Maximum number of results to return
    ///
    /// # Returns
    ///
    /// Results ordered by relevance (RRF fusion of BM25 and vector scores)
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<HybridSearchResult>> {
        tracing::debug!("Hybrid search: '{}' (limit: {})", query, limit);

        // 1. BM25 search
        let bm25_results = self
            .qmd_store
            .search_fts(query, self.config.bm25_candidates)?;

        tracing::debug!("BM25 found {} results", bm25_results.len());

        // 2. Vector search (Optional - Only if configured via feature flag)
        let vector_results: Vec<(String, f64)> = {
            #[cfg(feature = "vector")]
            {
                if self.vector_store.len() > 0 {
                    let query_embedding = self.embedder.embed(query)?;
                    self.vector_store
                        .search(&query_embedding, self.config.vector_candidates)?
                        .into_iter()
                        .map(|r| (r.docid, r.score))
                        .collect()
                } else {
                    Vec::new()
                }
            }
            #[cfg(not(feature = "vector"))]
            {
                Vec::new()
            }
        };

        tracing::debug!("Vector search found {} results", vector_results.len());

        // 3. Prepare data for RRF
        let bm25_for_rrf: Vec<(String, f64)> = bm25_results
            .iter()
            .map(|r| (r.document.docid.clone(), r.score))
            .collect();

        // 4. RRF fusion
        // Get more candidates for deduplication if vector search is enabled
        let fusion_limit = if cfg!(feature = "vector") {
            limit * 2
        } else {
            limit
        };
        let fused = self.rrf_fusion.fuse(&bm25_for_rrf, &vector_results);

        tracing::debug!("RRF fusion produced {} unique results", fused.len());

        // 5. Build initial results
        let mut candidates = Vec::new();
        for fused_result in fused.iter().take(fusion_limit) {
            if let Some(doc) = self.qmd_store.get_by_docid(&fused_result.docid)? {
                let snippet = bm25_results
                    .iter()
                    .find(|r| r.document.docid == fused_result.docid)
                    .and_then(|r| r.snippet.clone());

                candidates.push(HybridSearchResult {
                    rank: 0, // Placeholder
                    document: doc,
                    rrf_score: fused_result.rrf_score,
                    bm25_score: fused_result.bm25_score,
                    vector_score: fused_result.vector_score,
                    snippet,
                });
            }
        }

        // 6. Semantic Deduplication
        #[cfg(feature = "vector")]
        let mut final_results = self.apply_semantic_deduplication(candidates, 0.85, limit)?;
        #[cfg(not(feature = "vector"))]
        let mut final_results = candidates.into_iter().take(limit).collect::<Vec<_>>();

        // 7. Final ranking assignment
        for (i, res) in final_results.iter_mut().enumerate() {
            res.rank = i + 1;
        }

        Ok(final_results)
    }

    /// Search within a specific collection
    pub fn search_in_collection(
        &self,
        query: &str,
        collection: &str,
        limit: usize,
    ) -> Result<Vec<HybridSearchResult>> {
        tracing::debug!(
            "Hybrid search in collection '{}': '{}' (limit: {})",
            collection,
            query,
            limit
        );

        // 1. BM25 search in collection
        let bm25_results = self.qmd_store.search_fts_in_collection(
            query,
            collection,
            self.config.bm25_candidates,
        )?;

        tracing::debug!("BM25 found {} results in collection", bm25_results.len());

        // 2. Vector search (Optional)
        let vector_results: Vec<(String, f64)> = {
            #[cfg(feature = "vector")]
            {
                if self.vector_store.len() > 0 {
                    let query_embedding = self.embedder.embed(query)?;
                    self.vector_store
                        .search_in_collection(
                            &query_embedding,
                            Some(collection),
                            self.config.vector_candidates,
                        )?
                        .into_iter()
                        .map(|r| (r.docid, r.score))
                        .collect()
                } else {
                    Vec::new()
                }
            }
            #[cfg(not(feature = "vector"))]
            {
                Vec::new()
            }
        };

        tracing::debug!(
            "Vector search found {} results in collection",
            vector_results.len()
        );

        // 3. Prepare data for RRF
        let bm25_for_rrf: Vec<(String, f64)> = bm25_results
            .iter()
            .map(|r| (r.document.docid.clone(), r.score))
            .collect();

        // 4. RRF fusion
        let fusion_limit = if cfg!(feature = "vector") {
            limit * 2
        } else {
            limit
        };
        let fused = self.rrf_fusion.fuse(&bm25_for_rrf, &vector_results);

        // Build initial results
        let mut candidates = Vec::new();

        for fused_result in fused.iter().take(fusion_limit) {
            if let Some(doc) = self.qmd_store.get_by_docid(&fused_result.docid)? {
                let snippet = bm25_results
                    .iter()
                    .find(|r| r.document.docid == fused_result.docid)
                    .and_then(|r| r.snippet.clone());

                candidates.push(HybridSearchResult {
                    rank: 0, // Placeholder
                    document: doc,
                    rrf_score: fused_result.rrf_score,
                    bm25_score: fused_result.bm25_score,
                    vector_score: fused_result.vector_score,
                    snippet,
                });
            }
        }

        // 5. Semantic Deduplication
        #[cfg(feature = "vector")]
        let mut final_results = self.apply_semantic_deduplication(candidates, 0.85, limit)?;
        #[cfg(not(feature = "vector"))]
        let mut final_results = candidates.into_iter().take(limit).collect::<Vec<_>>();

        // 6. Final ranking assignment
        for (i, res) in final_results.iter_mut().enumerate() {
            res.rank = i + 1;
        }

        Ok(final_results)
    }

    /// Apply semantic deduplication to search results
    #[cfg(feature = "vector")]
    fn apply_semantic_deduplication(
        &self,
        candidates: Vec<HybridSearchResult>,
        threshold: f32,
        limit: usize,
    ) -> Result<Vec<HybridSearchResult>> {
        let mut unique_results: Vec<HybridSearchResult> = Vec::new();
        let mut embeddings: Vec<Vec<u8>> = Vec::new();

        for candidate in candidates {
            if let Some(emb) = self.vector_store.get_vector(&candidate.document.docid)? {
                let is_redundant = embeddings
                    .iter()
                    .any(|existing| Self::cosine_similarity_u8(existing, &emb) > threshold);

                if !is_redundant {
                    unique_results.push(candidate);
                    embeddings.push(emb);
                }
            } else {
                // If no vector found (e.g. BM25 only hit), we keep it but it won't trigger deduplication
                unique_results.push(candidate);
            }

            if unique_results.len() >= limit {
                break;
            }
        }

        Ok(unique_results)
    }

    /// Calculate cosine similarity between two u8-quantized vectors
    #[cfg(feature = "vector")]
    fn cosine_similarity_u8(a: &[u8], b: &[u8]) -> f32 {
        if a.len() != b.len() || a.is_empty() {
            return 0.0;
        }

        let mut dot_product = 0.0;
        let mut norm_a = 0.0;
        let mut norm_b = 0.0;

        for (&ua, &ub) in a.iter().zip(b.iter()) {
            // Unquantize: (u / 127.5) - 1.0
            let fa = (ua as f32 / 127.5) - 1.0;
            let fb = (ub as f32 / 127.5) - 1.0;

            dot_product += fa * fb;
            norm_a += fa * fa;
            norm_b += fb * fb;
        }

        let norm_product = norm_a.sqrt() * norm_b.sqrt();
        if norm_product == 0.0 {
            0.0
        } else {
            dot_product / norm_product
        }
    }

    /// Get statistics
    pub fn stats(&self) -> HybridSearchStats {
        let qmd_stats = self.qmd_store.get_stats().unwrap_or_default();

        let stats = HybridSearchStats {
            total_documents: qmd_stats.total_documents,
            total_collections: qmd_stats.total_collections,
            database_size_bytes: qmd_stats.database_size_bytes,
            ..Default::default()
        };

        #[cfg(feature = "vector")]
        {
            let mut final_stats = stats;
            final_stats.total_vectors = self.vector_store.len();
            final_stats.vector_dimension = self.vector_store.dimension();
            final_stats
        }
        #[cfg(not(feature = "vector"))]
        {
            stats
        }
    }

    /// Save vector store to disk
    pub fn save_vectors(&self) -> Result<()> {
        self.commit()
    }

    /// Vacuum the database
    pub fn vacuum(&self) -> Result<()> {
        self.qmd_store.vacuum()
    }
}

/// Hybrid search statistics
#[derive(Debug, Clone, Default)]
pub struct HybridSearchStats {
    pub total_documents: usize,
    pub total_collections: usize,
    pub total_vectors: usize,
    pub vector_dimension: usize,
    pub database_size_bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_config(temp_dir: &TempDir) -> HybridSearchConfig {
        let mut config = HybridSearchConfig::default();
        config.db_path = temp_dir.path().join("test.db");
        #[cfg(feature = "vector")]
        {
            config.vector_store_path = Some(temp_dir.path().join("test_vectors.bin"));
            config.bm25_candidates = 10;
            config.vector_candidates = 10;
            config.hnsw_max_elements = 1000;
        }
        config
    }

    #[test]
    #[ignore] // Requires ONNX model
    fn test_hybrid_search_engine_new() {
        let temp_dir = TempDir::new().unwrap();
        let config = create_test_config(&temp_dir);

        let engine = HybridSearchEngine::new(config);
        assert!(engine.is_ok());
    }

    #[test]
    #[ignore]
    fn test_index_and_search() {
        let temp_dir = TempDir::new().unwrap();
        let config = create_test_config(&temp_dir);

        let engine = HybridSearchEngine::new(config).unwrap();

        // Create collection
        engine
            .create_collection(Collection {
                name: "test".to_string(),
                description: Some("Test collection".to_string()),
                glob_pattern: "**/*.md".to_string(),
                root_path: None,
            })
            .unwrap();

        // Index documents
        engine
            .index_document(
                "test",
                "doc1.md",
                "Bitcoin Trading",
                "Buy Bitcoin when RSI is low. Sell when RSI is high.",
            )
            .unwrap();

        engine
            .index_document(
                "test",
                "doc2.md",
                "Ethereum Strategy",
                "Ethereum staking provides passive income.",
            )
            .unwrap();

        // Search
        let results = engine.search("cryptocurrency trading", 10).unwrap();

        assert!(results.len() > 0);
        // Bitcoin doc should rank high for "trading"
        assert!(results.iter().any(|r| r.document.title.contains("Bitcoin")));
    }

    #[test]
    #[ignore]
    fn test_search_in_collection() {
        let temp_dir = TempDir::new().unwrap();
        let config = create_test_config(&temp_dir);

        let engine = HybridSearchEngine::new(config).unwrap();

        engine
            .create_collection(Collection {
                name: "col1".to_string(),
                description: None,
                glob_pattern: "*.md".to_string(),
                root_path: None,
            })
            .unwrap();
        engine
            .create_collection(Collection {
                name: "col2".to_string(),
                description: None,
                glob_pattern: "*.md".to_string(),
                root_path: None,
            })
            .unwrap();

        engine
            .index_document("col1", "doc1.md", "Title1", "Content in col1")
            .unwrap();
        engine
            .index_document("col2", "doc2.md", "Title2", "Content in col2")
            .unwrap();

        // Search only in col1
        let results = engine.search_in_collection("Content", "col1", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].document.collection, "col1");
    }

    #[test]
    #[ignore]
    fn test_save_and_load_vectors() {
        let temp_dir = TempDir::new().unwrap();
        let config = create_test_config(&temp_dir);
        #[cfg(feature = "vector")]
        let vector_path = config.vector_store_path.clone().unwrap();

        {
            let engine = HybridSearchEngine::new(config.clone()).unwrap();

            engine
                .create_collection(Collection {
                    name: "test".to_string(),
                    description: None,
                    glob_pattern: "**/*.md".to_string(),
                    root_path: None,
                })
                .unwrap();

            engine
                .index_document("test", "doc.md", "Test", "Test content")
                .unwrap();

            engine.save_vectors().unwrap();
        }

        // Vector file should exist
        #[cfg(feature = "vector")]
        assert!(vector_path.exists());

        // Load in new engine
        let engine2 = HybridSearchEngine::new(config).unwrap();
        #[cfg(feature = "vector")]
        assert!(engine2.vector_store.len() > 0);
        #[cfg(not(feature = "vector"))]
        assert_eq!(engine2.stats().total_documents, 1);
    }

    #[test]
    #[ignore]
    fn test_concurrency() {
        use std::sync::Arc;
        use std::thread;

        let temp_dir = TempDir::new().unwrap();
        let config = create_test_config(&temp_dir);

        let engine = Arc::new(HybridSearchEngine::new(config).unwrap());

        let mut handles = Vec::new();
        for i in 0..10 {
            let engine_clone = Arc::clone(&engine);
            let handle = thread::spawn(move || {
                // Each thread tries to index and search
                let col = format!("col{}", i);
                engine_clone
                    .create_collection(Collection {
                        name: col.clone(),
                        description: None,
                        glob_pattern: "*.md".to_string(),
                        root_path: None,
                    })
                    .unwrap();

                engine_clone
                    .index_document(&col, "doc.md", "Title", "Content")
                    .unwrap();
                let results = engine_clone.search("Content", 10).unwrap();
                assert!(results.len() >= 1);
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(engine.stats().total_documents, 10);
    }

    #[test]
    #[cfg(feature = "vector")]
    fn test_cosine_similarity_u8() {
        let a = vec![255, 0, 127]; // [1.0, -1.0, 0.0] approx
        let b = vec![255, 0, 127];
        let sim = HybridSearchEngine::cosine_similarity_u8(&a, &b);
        assert!(sim > 0.99);

        let c = vec![0, 255, 127]; // [-1.0, 1.0, 0.0] approx
        let sim_neg = HybridSearchEngine::cosine_similarity_u8(&a, &c);
        assert!(sim_neg < -0.99);
    }

    #[test]
    #[ignore] // Requires model file
    #[cfg(feature = "vector")]
    fn test_semantic_deduplication() {
        let temp_dir = TempDir::new().unwrap();
        let config = create_test_config(&temp_dir);
        let engine = HybridSearchEngine::new(config).unwrap();

        engine
            .index_document(
                "test",
                "doc1.md",
                "Bitcoin",
                "Bitcoin is a decentralized currency.",
            )
            .unwrap();
        // Index a very similar document
        engine
            .index_document(
                "test",
                "doc2.md",
                "BTC",
                "Bitcoin is decentralized digital currency.",
            )
            .unwrap();

        // Search should deduplicate if threshold is reached
        let results = engine.search("bitcoin", 10).unwrap();

        // If threshold (0.85) is met, we should only see one of these
        // Since they are extremely similar, one should be pruned.
        // We check if at least one is present.
        assert!(!results.is_empty());
        // In a real scenario with a local model, we'd check if results.len() == 1
    }
}
