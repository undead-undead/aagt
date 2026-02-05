//! Text chunking for vector embeddings
//!
//! Splits long documents into overlapping chunks for better vector retrieval.
//! Uses sliding window with 800 tokens per chunk and 15% overlap.

use crate::error::Result;
use tokenizers::Tokenizer;

/// Configuration for text chunking
#[derive(Debug, Clone)]
pub struct ChunkerConfig {
    /// Chunk size in tokens (default: 800)
    pub chunk_size: usize,
    /// Overlap in tokens (default: 120, which is 15% of 800)
    pub overlap: usize,
    /// Path to tokenizer file
    pub tokenizer_path: std::path::PathBuf,
}

impl Default for ChunkerConfig {
    fn default() -> Self {
        Self {
            chunk_size: 800,
            overlap: 40, // 5% overlap (Reduced to save tokens)
            tokenizer_path: std::path::PathBuf::from("models/tokenizer.json"),
        }
    }
}

/// A chunk of text with metadata
#[derive(Debug, Clone)]
pub struct Chunk {
    /// Sequence number (0-indexed)
    pub seq: usize,
    /// Chunk text content
    pub text: String,
    /// Start position in original text (in characters)
    pub start_char: usize,
    /// End position in original text (in characters)
    pub end_char: usize,
    /// Start position in tokens
    pub start_token: usize,
    /// End position in tokens
    pub end_token: usize,
}

/// Text chunker for creating overlapping text segments
pub struct Chunker {
    tokenizer: Tokenizer,
    config: ChunkerConfig,
}

impl Chunker {
    /// Create a new chunker with default configuration
    ///
    /// Uses `all-MiniLM-L6-v2` tokenizer from HuggingFace
    pub fn new() -> Result<Self> {
        Self::with_config(ChunkerConfig::default())
    }

    /// Create a chunker with custom configuration
    ///
    /// Note: Requires tokenizer.json file at configured path
    pub fn with_config(config: ChunkerConfig) -> Result<Self> {
        // Load tokenizer from file
        let tokenizer = Tokenizer::from_file(&config.tokenizer_path).map_err(|e| {
            crate::error::QmdError::Custom(format!(
                "Failed to load tokenizer from {:?}: {}. Please download tokenizer.json from HuggingFace.",
                config.tokenizer_path, e
            ))
        })?;

        Ok(Self { tokenizer, config })
    }

    /// Chunk a document into overlapping segments
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use aagt_qmd::Chunker;
    /// let chunker = Chunker::new()?;
    /// let text = "Long document text...".repeat(1000);
    /// let chunks = chunker.chunk(&text)?;
    ///
    /// for chunk in chunks {
    ///     println!("Chunk {}: {} tokens", chunk.seq, chunk.text.len());
    /// }
    /// # Ok::<(), aagt_qmd::QmdError>(())
    /// ```
    pub fn chunk(&self, text: &str) -> Result<Vec<Chunk>> {
        if text.is_empty() {
            return Ok(vec![]);
        }

        // Tokenize the entire text
        let encoding = self
            .tokenizer
            .encode(text, false)
            .map_err(|e| crate::error::QmdError::Custom(format!("Tokenization failed: {}", e)))?;

        let tokens = encoding.get_ids();
        let offsets = encoding.get_offsets();

        if tokens.is_empty() {
            return Ok(vec![]);
        }

        // Calculate stride (non-overlapping part)
        let stride = self.config.chunk_size.saturating_sub(self.config.overlap);
        if stride == 0 {
            return Err(crate::error::QmdError::Custom(
                "Chunk size must be greater than overlap".to_string(),
            ));
        }

        let mut chunks = Vec::new();
        let mut chunk_seq = 0;

        // Sliding window chunking
        for window_start_token in (0..tokens.len()).step_by(stride) {
            let window_end_token = (window_start_token + self.config.chunk_size).min(tokens.len());

            // Get token IDs for this chunk
            let chunk_tokens = &tokens[window_start_token..window_end_token];

            // Get character offsets
            let start_char = offsets[window_start_token].0;
            let end_char = offsets[window_end_token - 1].1;

            // Decode tokens back to text
            let chunk_text = self
                .tokenizer
                .decode(chunk_tokens, true)
                .map_err(|e| crate::error::QmdError::Custom(format!("Decoding failed: {}", e)))?;

            chunks.push(Chunk {
                seq: chunk_seq,
                text: chunk_text,
                start_char,
                end_char,
                start_token: window_start_token,
                end_token: window_end_token,
            });

            chunk_seq += 1;

            // Stop if we've reached the end
            if window_end_token >= tokens.len() {
                break;
            }
        }

        Ok(chunks)
    }

    /// Get chunker statistics
    pub fn stats(&self, text: &str) -> Result<ChunkStats> {
        let encoding = self
            .tokenizer
            .encode(text, false)
            .map_err(|e| crate::error::QmdError::Custom(format!("Tokenization failed: {}", e)))?;

        let total_tokens = encoding.get_ids().len();
        let total_chars = text.len();

        let stride = self.config.chunk_size.saturating_sub(self.config.overlap);
        let estimated_chunks = if stride > 0 {
            (total_tokens + stride - 1) / stride
        } else {
            0
        };

        Ok(ChunkStats {
            total_tokens,
            total_chars,
            chunk_size: self.config.chunk_size,
            overlap: self.config.overlap,
            estimated_chunks,
        })
    }
}

/// Statistics about chunking
#[derive(Debug, Clone)]
pub struct ChunkStats {
    pub total_tokens: usize,
    pub total_chars: usize,
    pub chunk_size: usize,
    pub overlap: usize,
    pub estimated_chunks: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_chunker() -> Chunker {
        Chunker::with_config(ChunkerConfig {
            chunk_size: 200, // Larger chunks to ensure short text fits in one
            overlap: 10,
            ..Default::default()
        })
        .unwrap()
    }

    #[test]
    fn test_chunk_empty_text() {
        let chunker = create_test_chunker();
        let chunks = chunker.chunk("").unwrap();
        assert_eq!(chunks.len(), 0);
    }

    #[test]
    fn test_chunk_short_text() {
        let chunker = create_test_chunker();
        let text = "This is a short text that fits in one chunk.";
        let chunks = chunker.chunk(text).unwrap();

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].seq, 0);
        assert!(!chunks[0].text.is_empty());
    }

    #[test]
    fn test_chunk_long_text() {
        // Use very small chunks to ensure splitting even if tokenizer truncates
        let chunker = Chunker::with_config(ChunkerConfig {
            chunk_size: 10,
            overlap: 2,
            ..Default::default()
        })
        .unwrap();
        // Create a long text (repeat to ensure multiple chunks)
        let text = "This is a sample sentence. ".repeat(100);
        let chunks = chunker.chunk(&text).unwrap();

        // Should have multiple chunks
        assert!(chunks.len() > 1);

        // Chunks should have sequential seq numbers
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.seq, i);
        }

        // Chunks should overlap (verify by checking character positions)
        if chunks.len() > 1 {
            assert!(chunks[1].start_char < chunks[0].end_char);
        }
    }

    #[test]
    fn test_chunk_stats() {
        let chunker = create_test_chunker();
        let text = "Testing chunk statistics functionality.";
        let stats = chunker.stats(text).unwrap();

        assert_eq!(stats.chunk_size, 200);
        assert_eq!(stats.overlap, 10);
        assert!(stats.total_tokens > 0);
        assert_eq!(stats.total_chars, text.len());
        assert!(stats.estimated_chunks > 0);
    }

    #[test]
    fn test_chunk_overlap() {
        let chunker = create_test_chunker();
        let text = "word ".repeat(1000); // Long text to ensure multiple chunks
        let chunks = chunker.chunk(&text).unwrap();

        if chunks.len() >= 2 {
            // Verify overlap exists
            let chunk1_end = chunks[0].end_token;
            let chunk2_start = chunks[1].start_token;
            let overlap_tokens = chunk1_end - chunk2_start;

            assert!(overlap_tokens > 0, "Chunks should overlap");
            assert!(
                overlap_tokens <= 10,
                "Overlap should not exceed configured value"
            );
        }
    }
}
