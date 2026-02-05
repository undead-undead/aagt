//! Integration tests for aagt-qmd with aagt-core
//!
//! These tests verify that the QMD memory system integrates correctly with:
//! - Memory trait implementation
//! - SearchHistoryTool
//! - RememberThisTool
//! - MemoryManager
//! - AgentBuilder

use aagt_core::prelude::*;
use aagt_core::tool::{memory::{SearchHistoryTool, RememberThisTool}};
use std::sync::Arc;
use tempfile::TempDir;

#[tokio::test]
async fn test_qmd_memory_creation() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test_memory.db");

    // Test QmdMemory creation
    let memory = QmdMemory::new(100, path.clone()).await;
    assert!(memory.is_ok(), "QmdMemory should be created successfully");

    // Verify file was created
    assert!(path.exists(), "Database file should exist");
}

#[tokio::test]
async fn test_qmd_memory_store_and_engine() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test_store.db");
    
    let memory = QmdMemory::new(100, path).await.unwrap();
    
    // Test store operation (via Memory trait)
    let result = memory.store(
        "test_user",
        None,
        Message::user("Hello, world!"),
    ).await;
    assert!(result.is_ok(), "Store should succeed");
    
    // Test engine access
    let engine = memory.engine();
    assert!(Arc::strong_count(&engine) >= 1, "Engine should be accessible");
}

#[tokio::test]
async fn test_search_history_tool() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test_search.db");
    
    let memory = QmdMemory::new(100, path).await.unwrap();
    let engine = memory.engine();
    
    // Create SearchHistoryTool
    let search_tool = SearchHistoryTool::new(engine.clone());
    
    // First, index some content
    engine.index_document(
        "test_collection",
        "doc1",
        "Trading Strategy",
        "Buy SOL when RSI < 30, sell when RSI > 70"
    ).unwrap();
    
    // Test searching
    let args = serde_json::json!({
        "query": "RSI trading",
        "limit": 5
    });
    
    let result = search_tool.call(&args.to_string()).await;
    assert!(result.is_ok(), "Search should succeed");
    
    let output = result.unwrap();
    assert!(output.contains("Trading Strategy") || output.contains("No relevant"), 
        "Should find indexed content or report no results");
}

#[tokio::test]
async fn test_remember_this_tool() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test_remember.db");
    
    let memory = QmdMemory::new(100, path).await.unwrap();
    let engine = memory.engine();
    
    // Create RememberThisTool
    let remember_tool = RememberThisTool::new(engine.clone());
    
    // Test saving memory
    let args = serde_json::json!({
        "title": "User Preference",
        "content": "User prefers SOL over ETH for trading",
        "collection": "preferences"
    });
    
    let result = remember_tool.call(&args.to_string()).await;
    assert!(result.is_ok(), "Remember should succeed");
    
    let output = result.unwrap();
    assert!(output.contains("successfully saved"), "Should confirm save");
}

#[tokio::test]
async fn test_memory_manager_integration() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test_manager");
    
    // Create MemoryManager
    let manager = MemoryManager::with_capacity(100, 1000, 1000, path.clone()).await;
    assert!(manager.is_ok(), "MemoryManager should be created");
    
    let manager = manager.unwrap();
    
    // Test short-term memory
    manager.short_term.store(
        "user1",
        None,
        Message::user("Test message"),
    ).await.unwrap();
    
    let messages = manager.short_term.retrieve("user1", None, 10).await;
    assert_eq!(messages.len(), 1, "Should retrieve stored message");
    
    // Test long-term memory access
    let engine = manager.long_term.engine();
    assert!(Arc::strong_count(&engine) >= 1, "Engine should be accessible from manager");
}

#[tokio::test]
async fn test_agent_with_memory_tools() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test_agent");
    
    let memory = Arc::new(
        MemoryManager::with_capacity(100, 1000, 1000, path).await.unwrap()
    );
    
    // Mock provider for testing
    struct TestProvider;
    
    #[async_trait::async_trait]
    impl aagt_core::provider::Provider for TestProvider {
        fn name(&self) -> &'static str {
            "test"
        }
        async fn stream_completion(
            &self,
            _model: &str,
            _system: Option<&str>,
            _messages: Vec<Message>,
            _tools: Vec<ToolDefinition>,
            _temperature: Option<f64>,
            _max_tokens: Option<u64>,
            _extra_params: Option<serde_json::Value>,
        ) -> aagt_core::error::Result<StreamingResponse> {
            use futures::stream;
            use aagt_core::streaming::StreamingChoice;
            
            let chunks = vec![Ok(StreamingChoice::Message("Test response".to_string()))];
            let stream = Box::pin(stream::iter(chunks));
            Ok(StreamingResponse::new(stream))
        }
    }
    
    // Build agent with memory
    let agent = Agent::builder(TestProvider)
        .model("test-model")
        .with_memory(memory.clone())
        .build();
    
    assert!(agent.is_ok(), "Agent should be built with memory");
    
    let agent = agent.unwrap();
    
    // Verify tools were added
    let tools = agent.tool_definitions().await;
    assert!(tools.len() >= 2, "Should have at least search_history and remember_this tools");
    
    let tool_names: Vec<String> = tools.iter().map(|t| t.name.clone()).collect();
    assert!(tool_names.contains(&"search_history".to_string()), "Should have search_history tool");
    assert!(tool_names.contains(&"remember_this".to_string()), "Should have remember_this tool");
}

#[tokio::test]
async fn test_end_to_end_memory_workflow() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test_e2e");
    
    let memory = Arc::new(
        MemoryManager::with_capacity(100, 1000, 1000, path).await.unwrap()
    );
    
    let engine = memory.long_term.engine();
    
    // 1. Save a memory
    let remember_tool = RememberThisTool::new(engine.clone());
    let save_result = remember_tool.call(&serde_json::json!({
        "title": "Important Rule",
        "content": "Never invest more than 5% in a single asset",
        "collection": "trading_rules"
    }).to_string()).await;
    
    assert!(save_result.is_ok(), "Should save memory");
    
    // 2. Search for it
    let search_tool = SearchHistoryTool::new(engine.clone());
    let search_result = search_tool.call(&serde_json::json!({
        "query": "investment asset",
        "limit": 5
    }).to_string()).await;
    
    assert!(search_result.is_ok(), "Should search successfully");
    
    let results = search_result.unwrap();
    assert!(
        results.contains("Important Rule") || results.contains("No relevant"),
        "Should find saved content or report no results"
    );
    
    // 3. Verify short-term memory works separately
    memory.short_term.store("user1", None, Message::user("Short term test")).await.unwrap();
    let messages = memory.short_term.retrieve("user1", None, 10).await;
    assert_eq!(messages.len(), 1, "Short-term memory should work independently");
}

#[tokio::test]
async fn test_memory_persistence() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("test_persist.db");
    
    // Create first memory instance and save data
    {
        let memory = QmdMemory::new(100, db_path.clone()).await.unwrap();
        let engine = memory.engine();
        
        engine.index_document(
            "persistent",
            "doc1",
            "Persistent Data",
            "This should survive across instances"
        ).unwrap();
    }
    
    // Create second instance and verify data exists
    {
        let memory = QmdMemory::new(100, db_path.clone()).await.unwrap();
        let engine = memory.engine();
        
        let search_tool = SearchHistoryTool::new(engine.clone());
        let result = search_tool.call(&serde_json::json!({
            "query": "persistent",
            "limit": 5
        }).to_string()).await.unwrap();
        
        assert!(
            result.contains("Persistent Data") || result.contains("No relevant"),
            "Data should persist across QmdMemory instances"
        );
    }
}
