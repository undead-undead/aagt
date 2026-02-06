use aagt_core::prelude::*;
use aagt_core::agent::memory::QmdMemory;
use aagt_core::skills::tool::memory::{TieredSearchTool, FetchDocumentTool};
use std::sync::Arc;
use tempfile::TempDir;

#[tokio::test]
async fn test_semantic_deduplication_integration() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("test_dedup.db");
    
    let memory = QmdMemory::new(100, db_path.clone()).await.unwrap();
    let engine = memory.engine();
    
    // Index two semantically similar documents
    engine.index_document(
        "test",
        "doc1.md",
        "Bitcoin Info",
        "Bitcoin is a decentralized digital currency, without a central bank or single administrator."
    ).unwrap();
    
    engine.index_document(
        "test",
        "doc2.md",
        "BTC Info",
        "BTC is a peer-to-peer electronic cash system that works without a central authority."
    ).unwrap();
    
    println!("Stats after indexing: {:?}", engine.stats());

    // Search with a limit of 5.
    let results = engine.search("bitcoin", 5).unwrap();
    
    println!("Found {} results after deduplication for 'bitcoin'", results.len());
    for (i, res) in results.iter().enumerate() {
        println!("Result {}: {} (score: {})", i, res.document.title, res.rrf_score);
    }
    
    assert!(!results.is_empty(), "Should find at least one result for 'bitcoin'");
}

#[tokio::test]
async fn test_tiered_search_and_fetch() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("test_tiered.db");
    
    let memory = Arc::new(QmdMemory::new(100, db_path.clone()).await.unwrap());
    let engine = memory.engine();
    
    // Index a document with content
    let content = "The quick brown fox jumps over the lazy dog. This is a long document used to test tiered retrieval.";
    engine.index_document(
        "test",
        "fox.md",
        "Fox Story",
        content
    ).unwrap();
    
    // Manually add a summary for testing (since background worker might take time)
    engine.update_summary("test", "fox.md", "A story about a fox and a dog.").unwrap();
    
    // Test TieredSearchTool
    let tiered_tool = TieredSearchTool::new(memory.clone());
    let search_args = serde_json::json!({
        "query": "fox",
        "limit": 5
    });
    
    let search_result = tiered_tool.call(&search_args.to_string()).await.unwrap();
    assert!(search_result.contains("Fox Story"));
    assert!(search_result.contains("A story about a fox and a dog."));
    assert!(!search_result.contains("The quick brown fox jumps")); // Should NOT contain full text
    
    // Test FetchDocumentTool
    let fetch_tool = FetchDocumentTool::new(memory.clone());
    let fetch_args = serde_json::json!({
        "collection": "test",
        "path": "fox.md"
    });
    
    let fetch_result = fetch_tool.call(&fetch_args.to_string()).await.unwrap();
    assert!(fetch_result.contains("The quick brown fox jumps")); // Should contain full text
}
