//! # AAGT-QMD: QMD-Inspired Hybrid Search Engine
//!
//! A Rust implementation of QMD (Query Markup Documents) for AAGT, providing:
//! - **Content-Addressable Storage**: Automatic deduplication via SHA-256 hashing
//! - **FTS5 Full-Text Search**: Fast BM25 keyword search with snippet extraction
//! - **Vector Similarity Search**: Semantic search using dense embeddings (Phase 2)
//! - **Hybrid Search**: RRF fusion of BM25 and vector search (Phase 2)
//! - **Virtual Path System**: Unified path abstraction across collections
//! - **SQLite Storage**: Zero-dependency local database with WAL mode
//!
//! ## Quick Start (Phase 1: BM25/FTS5)
//!
//! ```rust
//! use aagt_qmd::{QmdStore, Collection};
//!
//! # fn main() -> aagt_qmd::Result<()> {
//! // Create a new store
//! let mut store = QmdStore::new("knowledge_base.db")?;
//!
//! // Create a collection
//! store.create_collection(Collection {
//!     name: "trading".to_string(),
//!     description: Some("Trading strategies and analysis".to_string()),
//!     glob_pattern: "**/*.md".to_string(),
//!     root_path: None,
//! })?;
//!
//! // Store a document
//! let doc = store.store_document(
//!     "trading",
//!     "strategies/sol.md",
//!     "SOL Trading Strategy",
//!     "Buy SOL when RSI < 30, sell when RSI > 70"
//! )?;
//!
//! println!("Stored document with docid: #{}", doc.docid);
//!
//! // Full-text search
//! let results = store.search_fts("RSI trading", 10)?;
//! for result in results {
//!     println!("Found: {} (score: {:.2})", result.document.title, result.score);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Phase 2: Hybrid Search (BM25 + Vector)
//!
//! ```rust,no_run
//! use aagt_qmd::{HybridSearchEngine, HybridSearchConfig};
//!
//! # fn main() -> aagt_qmd::Result<()> {
//! // Create hybrid search engine
//! let mut engine = HybridSearchEngine::new(HybridSearchConfig::default())?;
//!
//! // Index documents (automatically chunks and embeds)
//! engine.index_document(
//!     "trading",
//!     "bear_market.md",
//!     "熊市获利策略",
//!     "在熊市中可以通过抄底、DCA定投等方式获利。重要的是控制仓位。"
//! )?;
//!
//! // Hybrid search (BM25 + Vector + RRF)
//! let results = engine.search("如何在熊市赚钱", 10)?;
//!
//! for result in results {
//!     println!("{}. {} (RRF: {:.4})",
//!         result.rank,
//!         result.document.title,
//!         result.rrf_score
//!     );
//!     if let Some(bm25) = result.bm25_score {
//!         println!("    BM25: {:.2}", bm25);
//!     }
//!     if let Some(vec_score) = result.vector_score {
//!         println!("    Vector: {:.2}", vec_score);
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Architecture
//!
//! ```text
//! Phase 1: BM25/FTS5                  Phase 2: Hybrid Search
//! ┌──────────────────────┐           ┌──────────────────────┐
//! │   QmdStore (BM25)    │           │ HybridSearchEngine   │
//! │  - Keyword matching  │           │  - Semantic search   │
//! │  - FTS5 index        │   +       │  - Vector embeddings │
//! │  - Snippet extract   │           │  - RRF fusion        │
//! └──────────────────────┘           └──────────────────────┘
//!              │                              │
//!              ▼                              ▼
//!     ┌────────────────┐            ┌────────────────┐
//!     │ SQLite + FTS5  │            │ HNSW + ONNX    │
//!     └────────────────┘            └────────────────┘
//! ```
//!
//! ## Features
//!
//! - **fts** (default): FTS5 full-text search (Phase 1)
//! - **vector**: Vector similarity search + hybrid search (Phase 2)
//! - **embeddings**: Alternative pure Rust embeddings via Candle (experimental)
//! - **full**: FTS + Vector (recommended for production)

// Phase 1 modules (always available)
pub mod content_hash;
pub mod error;
pub mod store;
pub mod virtual_path;

// Phase 2 modules (vector feature)
pub mod hybrid_search;
pub mod rrf;

// Phase 2 modules (vector feature)
#[cfg(feature = "vector")]
pub mod chunker;
#[cfg(feature = "vector")]
pub mod embedder;
#[cfg(feature = "vector")]
pub mod vector_store;

// Re-exports: Phase 1
pub use content_hash::{get_docid, hash_content, normalize_docid, validate_docid};
pub use error::{QmdError, Result};
pub use store::{Collection, Document, QmdStore, SearchResult, StoreStats};
pub use virtual_path::VirtualPath;

// Re-exports: Phase 2
pub use hybrid_search::{
    HybridSearchConfig, HybridSearchEngine, HybridSearchResult, HybridSearchStats,
};
pub use rrf::{FusedResult, RrfConfig, RrfFusion};

// Re-exports: Phase 2
#[cfg(feature = "vector")]
pub use chunker::{Chunk, ChunkStats, Chunker, ChunkerConfig};
#[cfg(feature = "vector")]
pub use embedder::{Embedder, EmbedderConfig};
#[cfg(feature = "vector")]
pub use vector_store::{VectorEntry, VectorSearchResult, VectorStore};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_exports() {
        // Verify all main types are accessible
        let _: Result<()> = Ok(());
        let _vpath = VirtualPath::parse("aagt://test/doc.md");
        let _hash = hash_content("test");
        let _docid = get_docid("abc123def456");
    }

    #[test]
    #[cfg(feature = "vector")]
    fn test_vector_module_exports() {
        // Verify Phase 2 types are accessible
        let _config = ChunkerConfig::default();
        let _emb_config = EmbedderConfig::default();
        let _rrf_config = RrfConfig::default();
    }
}
