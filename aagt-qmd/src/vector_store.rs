//! Vector storage and similarity search using HNSW index
//!
//! Provides efficient k-NN search for dense vectors using Hierarchical Navigable Small World graphs.

use crate::error::{QmdError, Result};

use hnsw_rs::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::Path;

use std::sync::RwLock;

/// A vector entry with metadata (Quantized to u8)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorEntry {
    /// Document ID (short hash)
    pub docid: String,
    /// Collection name
    pub collection: String,
    /// Chunk sequence number
    pub chunk_seq: usize,
    /// Quantized vector embedding (u8)
    /// Range [-1.0, 1.0] mapped to [0, 255]
    pub embedding: Vec<u8>,
}

/// Vector search result
#[derive(Debug, Clone)]
pub struct VectorSearchResult {
    /// Document ID
    pub docid: String,
    /// Collection
    pub collection: String,
    /// Chunk sequence number
    pub chunk_seq: usize,
    /// Similarity score (approximate)
    pub score: f64,
}

/// Vector store using HNSW index with u8 quantization
pub struct VectorStore {
    /// Vector entries (Source of Truth)
    entries: RwLock<Vec<VectorEntry>>,
    /// HNSW index (u8)
    hnsw: RwLock<Hnsw<'static, u8, DistU8L2>>,
    /// Dimension of vectors
    dimension: usize,
    /// Max elements for HNSW
    max_elements: usize,
    /// Dirty flag
    dirty: RwLock<bool>,
}

impl VectorStore {
    pub fn new(dimension: usize, max_elements: usize) -> Self {
        // M=16, ef_construction=200
        let hnsw = Hnsw::new(16, max_elements, 16, 200, DistU8L2);
        Self {
            entries: RwLock::new(Vec::new()),
            hnsw: RwLock::new(hnsw),
            dimension,
            max_elements,
            dirty: RwLock::new(false),
        }
    }

    /// Quantize f32 vector to u8
    /// Assumes input is normalized to roughly [-1.0, 1.0]
    fn quantize(vec: &[f32]) -> Vec<u8> {
        vec.iter()
            .map(|&x| {
                // Map [-1.0, 1.0] -> [0, 255]
                // +1.0 -> [0.0, 2.0]
                // * 127.5 -> [0.0, 255.0]
                ((x + 1.0) * 127.5).clamp(0.0, 255.0) as u8
            })
            .collect()
    }

    /// Add a vector (auto-quantizes)
    pub fn add(
        &self,
        collection: impl Into<String>,
        docid: impl Into<String>,
        chunk_seq: usize,
        embedding: Vec<f32>,
    ) -> Result<()> {
        if embedding.len() != self.dimension {
            return Err(QmdError::Custom(format!(
                "Dimension mismatch: expected {}, got {}",
                self.dimension,
                embedding.len()
            )));
        }

        let quantized = Self::quantize(&embedding);

        let mut entries = self
            .entries
            .write()
            .map_err(|_| QmdError::Custom("Lock poisoned".to_string()))?;
        let hnsw = self
            .hnsw
            .write()
            .map_err(|_| QmdError::Custom("Lock poisoned".to_string()))?;
        let mut dirty = self
            .dirty
            .write()
            .map_err(|_| QmdError::Custom("Lock poisoned".to_string()))?;

        let idx = entries.len();
        hnsw.parallel_insert(&[(&quantized, idx)]);

        entries.push(VectorEntry {
            docid: docid.into(),
            collection: collection.into(),
            chunk_seq,
            embedding: quantized,
        });

        *dirty = true;
        Ok(())
    }

    /// Search (auto-quantizes query)
    pub fn search(&self, query_embedding: &[f32], k: usize) -> Result<Vec<VectorSearchResult>> {
        self.search_in_collection(query_embedding, None, k)
    }

    /// Search in a specific collection (auto-quantizes query)
    pub fn search_in_collection(
        &self,
        query_embedding: &[f32],
        collection: Option<&str>,
        k: usize,
    ) -> Result<Vec<VectorSearchResult>> {
        if query_embedding.len() != self.dimension {
            return Err(QmdError::Custom("Dimension mismatch".to_string()));
        }

        let entries = self
            .entries
            .read()
            .map_err(|_| QmdError::Custom("Lock poisoned".to_string()))?;
        if entries.is_empty() {
            return Ok(Vec::new());
        }

        let hnsw = self
            .hnsw
            .read()
            .map_err(|_| QmdError::Custom("Lock poisoned".to_string()))?;
        let query_u8 = Self::quantize(query_embedding);

        // If we have a collection filter, we increase search depth to ensure we find enough candidates
        let search_k = if collection.is_some() {
            (k * 4).max(100)
        } else {
            k
        };
        let ef_search = (search_k * 2).max(50);

        let neighbors = hnsw.search(&query_u8, search_k, ef_search);

        let mut results = Vec::new();
        for neighbor in neighbors {
            if neighbor.d_id < entries.len() {
                let entry = &entries[neighbor.d_id];

                // Post-filtering by collection
                if let Some(col) = collection {
                    if entry.collection != col {
                        continue;
                    }
                }

                let score = 1.0 / (1.0 + neighbor.distance as f64);

                results.push(VectorSearchResult {
                    docid: entry.docid.clone(),
                    collection: entry.collection.clone(),
                    chunk_seq: entry.chunk_seq,
                    score,
                });

                if results.len() >= k {
                    break;
                }
            }
        }

        Ok(results)
    }

    pub fn len(&self) -> usize {
        self.entries.read().map(|e| e.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn is_dirty(&self) -> bool {
        *self.dirty.read().unwrap()
    }

    pub fn dimension(&self) -> usize {
        self.dimension
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        if !self.is_dirty() {
            return Ok(());
        }

        let path = path.as_ref();
        let entries = self
            .entries
            .read()
            .map_err(|_| QmdError::Custom("Lock poisoned".to_string()))?;

        let data = VectorStoreData {
            entries: entries.clone(),
            dimension: self.dimension,
        };

        let tmp_path = path.with_extension("tmp");
        {
            let file = std::fs::File::create(&tmp_path).map_err(QmdError::Io)?;
            let writer = std::io::BufWriter::new(file);
            bincode::serialize_into(writer, &data)
                .map_err(|e| QmdError::Custom(format!("Serialization failed: {}", e)))?;
        }

        std::fs::rename(tmp_path, path).map_err(QmdError::Io)?;

        {
            let mut dirty = self
                .dirty
                .write()
                .map_err(|_| QmdError::Custom("Lock poisoned".to_string()))?;
            *dirty = false;
        }
        Ok(())
    }

    pub fn save_force(&self, path: impl AsRef<Path>) -> Result<()> {
        {
            let mut dirty = self
                .dirty
                .write()
                .map_err(|_| QmdError::Custom("Lock poisoned".to_string()))?;
            *dirty = true;
        }
        self.save(path)
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let file = std::fs::File::open(path).map_err(QmdError::Io)?;
        let reader = std::io::BufReader::new(file);

        let store_data: VectorStoreData = bincode::deserialize_from(reader)
            .map_err(|e| QmdError::Custom(format!("Deserialization failed: {}", e)))?;

        let store = Self::new(store_data.dimension, store_data.entries.len().max(100));
        {
            let mut entries_lock = store.entries.write().unwrap();
            let hnsw_lock = store.hnsw.write().unwrap();
            for entry in store_data.entries {
                let idx = entries_lock.len();
                hnsw_lock.parallel_insert(&[(&entry.embedding, idx)]);
                entries_lock.push(entry);
            }
        }
        // dirty is false by default in new(), which is correct after load
        Ok(store)
    }

    pub fn clear(&self) {
        if let Ok(mut entries) = self.entries.write() {
            entries.clear();
        }
        if let Ok(mut hnsw) = self.hnsw.write() {
            *hnsw = Hnsw::new(16, self.max_elements, 16, 200, DistU8L2);
        }
        if let Ok(mut dirty) = self.dirty.write() {
            *dirty = true;
        }
    }
}

/// Serializable vector store data (u8)
#[derive(Serialize, Deserialize)]
struct VectorStoreData {
    entries: Vec<VectorEntry>,
    dimension: usize,
}

/// L2 Squared Distance for u8
///
/// For normalized vectors (living on a hypersphere),
/// Euclidean Distance order is equivalent to Cosine Distance order.
/// We calculate sum((a-b)^2).
#[derive(Clone, Copy)]
struct DistU8L2;

impl Distance<u8> for DistU8L2 {
    fn eval(&self, a: &[u8], b: &[u8]) -> f32 {
        let mut sum_sq_diff = 0u32;
        // Manual loop for u8 differences to avoid overflow
        for (&x, &y) in a.iter().zip(b.iter()) {
            let diff = if x > y { x - y } else { y - x };
            sum_sq_diff += (diff as u32) * (diff as u32);
        }
        sum_sq_diff as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vector_store_new() {
        let store = VectorStore::new(384, 1000);
        assert_eq!(store.dimension(), 384);
        assert_eq!(store.len(), 0);
        assert!(store.is_empty());
    }

    #[test]
    fn test_add_and_search() {
        let store = VectorStore::new(3, 100);

        // Add some vectors
        let vec1 = vec![1.0, 0.0, 0.0];
        let vec2 = vec![0.0, 1.0, 0.0];
        let vec3 = vec![0.9, 0.1, 0.0]; // Similar to vec1

        store.add("trading", "doc1", 0, vec1.clone()).unwrap();
        store.add("trading", "doc2", 0, vec2.clone()).unwrap();
        store.add("trading", "doc3", 0, vec3.clone()).unwrap();

        assert_eq!(store.len(), 3);

        // Search with vec1 - should find doc1 and doc3
        let results = store.search(&vec1, 2).unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].docid, "doc1"); // Exact match
        assert!(results[0].score > 0.99);
    }

    #[test]
    fn test_search_collection_filter() {
        let store = VectorStore::new(3, 100);

        store.add("col1", "doc1", 0, vec![1.0, 0.0, 0.0]).unwrap();
        store.add("col2", "doc2", 0, vec![1.0, 0.0, 0.0]).unwrap();

        // Search in col1 only
        let results = store
            .search_in_collection(&vec![1.0, 0.0, 0.0], Some("col1"), 10)
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].collection, "col1");
        assert_eq!(results[0].docid, "doc1");
    }

    #[test]
    fn test_dimension_mismatch() {
        let store = VectorStore::new(384, 100);

        let wrong_vec = vec![1.0, 2.0]; // Wrong dimension
        let result = store.add("test", "doc1", 0, wrong_vec);

        assert!(result.is_err());
    }

    #[test]
    fn test_search_empty_store() {
        let store = VectorStore::new(384, 100);
        let query = vec![0.0; 384];

        let results = store.search(&query, 10).unwrap();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn test_save_and_load() {
        let temp_file = tempfile::NamedTempFile::new().unwrap();
        let path = temp_file.path();

        // Create and populate store
        let store = VectorStore::new(3, 100);
        store
            .add("trading", "doc1", 0, vec![1.0, 0.0, 0.0])
            .unwrap();
        store
            .add("trading", "doc2", 1, vec![0.0, 1.0, 0.0])
            .unwrap();

        // Save
        store.save(path).unwrap();

        // Load
        let loaded = VectorStore::load(path).unwrap();

        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded.dimension(), 3);

        // Verify search still works
        let results = loaded.search(&vec![1.0, 0.0, 0.0], 1).unwrap();
        assert_eq!(results[0].docid, "doc1");
    }

    #[test]
    fn test_clear() {
        let store = VectorStore::new(3, 100);
        store
            .add("trading", "doc1", 0, vec![1.0, 0.0, 0.0])
            .unwrap();

        assert_eq!(store.len(), 1);

        store.clear();

        assert_eq!(store.len(), 0);
        assert!(store.is_empty());
    }

    #[test]
    fn test_similarity_ranking() {
        let store = VectorStore::new(3, 100);

        // Normalize vectors for proper cosine similarity
        let normalize = |v: Vec<f32>| -> Vec<f32> {
            let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            v.iter().map(|x| x / norm).collect()
        };

        let vec_anchor = normalize(vec![1.0, 0.0, 0.0]);
        let vec_similar = normalize(vec![0.9, 0.1, 0.0]);
        let vec_different = normalize(vec![0.0, 1.0, 0.0]);

        store.add("col", "anchor", 0, vec_anchor.clone()).unwrap();
        store.add("col", "similar", 0, vec_similar.clone()).unwrap();
        store
            .add("col", "different", 0, vec_different.clone())
            .unwrap();

        let query = vec_anchor;
        let results = store.search(&query, 3).unwrap();

        // Results should be ordered by similarity
        assert_eq!(results[0].docid, "anchor");
        assert_eq!(results[1].docid, "similar");
        assert_eq!(results[2].docid, "different");

        // Check score ordering
        assert!(results[0].score > results[1].score);
        assert!(results[1].score > results[2].score);
    }
}
