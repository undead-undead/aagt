use aagt_core::risk::{RiskManager, RiskConfig, TradeContext, FileRiskStore, RiskStateStore};
use aagt_core::store::file::{FileStore, FileStoreConfig};
use aagt_core::rag::VectorStore;
use std::sync::Arc;
use std::path::PathBuf;
use std::collections::HashMap;

#[tokio::test]
async fn verify_risk_persistence_fix() {
    let path = PathBuf::from("test_risk_persist.json");
    if path.exists() { std::fs::remove_file(&path).unwrap(); }
    
    let store = Arc::new(FileRiskStore::new(path.clone()));
    let config = RiskConfig::default();

    // 1. Create Manager A
    let manager_a = RiskManager::with_config(config.clone(), store.clone()).await.expect("Failed to create manager A");
    
    // 2. Commit a trade
    manager_a.commit_trade("user_verify", 5000.0).await.expect("Commit failed");
    
    // 3. Create Manager B (simulate restart) - Should load state automatically
    let manager_b = RiskManager::with_config(config.clone(), store.clone()).await.expect("Failed to create manager B");
    
    // 4. Check remaining limit (Should be 50k - 5k = 45k)
    let remaining = manager_b.remaining_daily_limit("user_verify").await;
    
    assert_eq!(remaining, 45_000.0, "Risk state was not loaded correctly on restart!");
    
    // Cleanup
    if path.exists() { std::fs::remove_file(&path).unwrap(); }
}

#[tokio::test]
async fn verify_filestore_metadata_optimization() {
    let path = PathBuf::from("test_filestore_oom.jsonl");
    if path.exists() { 
        std::fs::remove_file(&path).ok(); 
        std::fs::remove_file(path.with_extension("index")).ok(); 
        std::fs::remove_file(path.with_extension("index_v2")).ok(); 
    }

    let config = FileStoreConfig::new(path.clone());
    let store = FileStore::new(config).await.expect("Failed to create store");

    // 1. Store document with HUGE metadata
    let huge_content = "x".repeat(1024 * 10); // 10KB
    let mut metadata = HashMap::new();
    metadata.insert("page_content".to_string(), huge_content.clone());
    metadata.insert("safe_key".to_string(), "safe_value".to_string());
    
    let id = store.store("content", metadata).await.expect("Store failed");

    // 2. Check Index (via search finding it)
    let results = store.search("content", 1).await.expect("Search failed");
    assert_eq!(results.len(), 1);
    
    // 3. Verify retrieved document HAS the content (loaded from disk)
    // Note: The search result metadata comes from reading the file, so it SHOULD have the content.
    assert_eq!(results[0].metadata.get("page_content"), Some(&huge_content));
    
    // 4. Access internal index to verify it DOES NOT have the content (Optimization check)
    // We can't access private fields directly in integration test, 
    // but we can trust the code if the logic holds. 
    // Alternatively, we can use `find_by_metadata` on the excluded key.
    // IF the key is excluded from index, `find_by_metadata` on that key should FAIL or return nothing 
    // (since find_by_metadata usually scans the index).
    
    let findings = store.find_by_metadata("page_content", &huge_content).await;
    assert_eq!(findings.len(), 0, "Huge metadata key SHOULD have been filtered from index!");

    let findings_safe = store.find_by_metadata("safe_key", "safe_value").await;
    assert_eq!(findings_safe.len(), 1, "Safe metadata key SHOULD exist in index");

    // Cleanup
    if path.exists() { 
        std::fs::remove_file(&path).ok(); 
        std::fs::remove_file(path.with_extension("index")).ok(); 
        std::fs::remove_file(path.with_extension("index_v2")).ok(); 
    }
}

#[tokio::test]
async fn verify_long_term_memory_store_fix() {
    use aagt_core::memory::{LongTermMemory, Memory};
    use aagt_core::message::Message;

    let path = PathBuf::from("test_ltm_store.jsonl");
    if path.exists() { std::fs::remove_file(&path).ok(); std::fs::remove_file(path.with_extension("index")).ok(); }
    
    let memory = LongTermMemory::new(100, path.clone()).await.expect("Failed to create LTM");
    
    // 1. Call store (from trait) - Async call
    memory.store("user_ltm", None, Message::user("I remember this!")).await.expect("Store failed");
    
    // 2. No need to wait blindly, await guarantees completion (mostly, assuming internal implementation awaits)
    // But since FileStore might use spawn_blocking internally, we are good once the future returns.
    
    // 3. Verify it was stored using retrieve_recent
    let recent = memory.retrieve_recent("user_ltm", None, 1000).await;
    assert_eq!(recent.len(), 1, "Message should have been stored persistentl!");
    assert_eq!(recent[0].content, "I remember this!");
    
    // Cleanup
    if path.exists() { std::fs::remove_file(&path).ok(); std::fs::remove_file(path.with_extension("index")).ok(); }
}
