# AAGT-QMD: Advanced Knowledge & Persistence Engine

`aagt-qmd` is the high-performance storage and retrieval core of the AAGT framework. It combines classical information retrieval with modern semantic search and content-addressable storage.

---

## Key Architectures

### 1. Hybrid Search (Phase 2)
The engine utilizes **Reciprocal Rank Fusion (RRF)** to combine two world-class search techniques:
- **BM25 (FTS5)**: Traditional keyword-based search via SQLite, excellent for exact matches and specific technical terms.
- **Dense Vector Search**: Semantic similarity search using `candle-core` (BERT embeddings) and `hnsw_rs`, perfect for "finding meaning" even if keywords don't match.

### 2. Content-Addressable Storage (CAS)
- **Automatic Deduplication**: Documents are indexed by the SHA-256 hash of their content. If multiple files share the same content, only one copy is stored in the `content` table.
- **Minimal Footprint**: Dramatically reduces database size when many documents share boilerplate or historical data.

### 3. Agent Session Persistence
- **Stateful Resume**: Implements `AgentSession` storage in SQLite, allowing agents to be serialized, stopped, and resumed later without losing their internal state or conversation chain.
- **Checkpointing**: Supports robust checkpointing during multi-step reasoning.

### 4. Active Indexing
- **Real-time Monitoring**: Uses the `notify` crate to watch local directories. When an `.md` file is changed, the engine automatically re-chunks, re-embeds, and re-indexes it in the background.

---

## Features & Feature Flags

- **`fts`** (Default): Keyword-based full-text search using SQLite FTS5.
- **`vector`**: Enables BERT embeddings, HNSW indexing, and semantic similarity search (Requires ONNX/Safetensors models).
- **`full`**: Enables all features for industrial-grade RAG (Retrieval-Augmented Generation).

---

## Performance & Scaling
- **WAL Mode**: SQLite is configured in Write-Ahead Logging mode for safe concurrent reads and writes.
- **Quantization**: High-dimensional vectors are managed with efficient memory layouts to support low-latency retrieval on VPS environments.

---

## Source Structure
- `store.rs`: SQLite schema, trigger logic, and document persistence.
- `embedder.rs`: BERT model integration via Candle.
- `vector_store.rs`: HNSW index management.
- `rrf.rs`: Ranking fusion algorithm (Reciprocal Rank Fusion).
- `watcher.rs`: File system event monitoring for auto-indexing.
- `agent_memory.rs`: The bridge to AAGT's core `Memory` trait.
