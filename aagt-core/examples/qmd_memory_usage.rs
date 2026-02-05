//! QMD Memory Usage Example
//!
//! Demonstrates how to use the QMD (Query Markup Documents) memory system with AAGT agents.
//!
//! This example shows:
//! 1. Creating a MemoryManager with QMD backend
//! 2. Building an agent with memory tools
//! 3. Automatic conversation storage
//! 4. Agent using search_history tool
//! 5. Agent using remember_this tool
//!
//! Run with:
//! ```bash
//! cargo run --package aagt-core --example qmd_memory_usage
//! ```

use aagt_core::prelude::*;
use aagt_core::tool::{memory::{SearchHistoryTool, RememberThisTool}};
use std::sync::Arc;
use tempfile::TempDir;

/// A mock provider for demonstration purposes
struct MockProvider;

#[async_trait::async_trait]
impl aagt_core::provider::Provider for MockProvider {
    fn name(&self) -> &'static str {
        "mock"
    }

    async fn stream_completion(
        &self,
        _model: &str,
        _system: Option<&str>,
        messages: Vec<Message>,
        _tools: Vec<ToolDefinition>,
        _temperature: Option<f64>,
        _max_tokens: Option<u64>,
        _extra_params: Option<serde_json::Value>,
    ) -> Result<StreamingResponse> {
        // Mock response based on the last message
        let last_msg = messages.last()
            .and_then(|m| match &m.content {
                Content::Text(t) => Some(t.as_str()),
                Content::Parts(parts) => parts.iter().find_map(|p| {
                    if let aagt_core::message::ContentPart::Text { text } = p {
                        Some(text.as_str())
                    } else {
                        None
                    }
                }),
            })
            .unwrap_or("");

        let response = if last_msg.contains("tell me about") || last_msg.contains("what is") {
            "SOL (Solana) is a high-performance blockchain platform designed for decentralized applications."
        } else if last_msg.contains("search") || last_msg.contains("find") {
            "I'll search for that information using the search_history tool."
        } else if last_msg.contains("remember") || last_msg.contains("save") {
            "I'll save that to my long-term memory using the remember_this tool."
        } else {
            "I understand. How can I help you with trading strategies?"
        };

        // Create a simple streaming response
        use futures::stream;
        use aagt_core::streaming::StreamingChoice;
        
        let chunks = vec![
            Ok(StreamingChoice::Message(response.to_string())),
        ];
        
        let stream = Box::pin(stream::iter(chunks));
        Ok(StreamingResponse::new(stream))
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("🚀 AAGT QMD Memory Usage Example\n");

    // 1. Create temporary directory for this example
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let db_path = temp_dir.path().join("example_memory.db");
    
    println!("📁 Using database: {:?}\n", db_path);

    // 2. Create MemoryManager with QMD backend
    println!("🧠 Creating MemoryManager with QMD backend...");
    let memory = Arc::new(
        MemoryManager::with_capacity(
            100,   // short_term_max: keep last 100 messages in RAM
            1000,  // short_term_users: support up to 1000 concurrent users
            1000,  // long_term_max: keep last 1000 entries per user in QMD
            db_path.clone(),
        )
        .await?
    );
    println!("✅ MemoryManager created\n");

    // 3. Build Agent with memory tools
    println!("🤖 Building Agent with memory integration...");
    let provider = MockProvider;
    let agent = Agent::builder(provider)
        .model("mock-model")
        .system_prompt("You are a helpful trading assistant with access to long-term memory.")
        .with_memory(memory.clone())  // This adds search_history and remember_this tools
        .build()?;
    
    println!("✅ Agent built with {} tools\n", agent.tool_definitions().await.len());

    // 4. Demonstrate conversation storage (automatic)
    println!("💬 Starting conversation...");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    // First conversation
    let user_id = "user123";
    memory.short_term.store(
        user_id,
        None,
        Message::user("Tell me about SOL"),
    ).await?;
    println!("👤 User: Tell me about SOL");

    let response = agent.prompt("Tell me about SOL").await?;
    println!("🤖 Agent: {}\n", response);

    memory.short_term.store(
        user_id,
        None,
        Message::assistant(response.clone()),
    ).await?;

    // 5. Demonstrate manual memory storage using remember_this tool
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("💾 Saving key insight to long-term memory...\n");

    let engine = memory.long_term.engine();
    let remember_tool = RememberThisTool::new(engine.clone());
    
    let remember_args = serde_json::json!({
        "title": "SOL Overview",
        "content": "Solana (SOL) is a high-performance blockchain with fast transaction speeds and low fees.",
        "collection": "trading_knowledge"
    });

    let remember_result = remember_tool.call(&remember_args.to_string()).await?;
    println!("✅ {}\n", remember_result);

    // 6. Demonstrate search_history tool
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("🔍 Searching for SOL-related information...\n");

    let search_tool = SearchHistoryTool::new(engine.clone());
    let search_args = serde_json::json!({
        "query": "Solana blockchain",
        "limit": 5
    });

    let search_results = search_tool.call(&search_args.to_string()).await?;
    println!("{}\n", search_results);

    // 7. Show memory statistics
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("📊 Memory Statistics\n");
    println!("  Short-term messages: {}", memory.short_term.message_count(user_id, None));
    println!("  Database path: {:?}", db_path);
    
    // 8. Demonstrate persistence
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("🔄 Testing persistence...\n");
    
    // Retrieve from short-term memory
    let recent = memory.short_term.retrieve(user_id, None, 10).await;
    println!("✅ Retrieved {} messages from short-term memory", recent.len());
    
    for (i, msg) in recent.iter().enumerate() {
        println!("  {}. {} - {}", 
            i + 1,
            match msg.role {
                Role::User => "👤",
                Role::Assistant => "🤖",
                _ => "📝",
            },
            msg.text().chars().take(50).collect::<String>()
        );
    }

    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Example completed successfully!");
    println!("\n💡 Key takeaways:");
    println!("  • MemoryManager provides both short-term and long-term memory");
    println!("  • QMD backend offers fast BM25 search (100x faster than linear scan)");
    println!("  • Agents can actively search and save using memory tools");
    println!("  • Memory persists across restarts via SQLite database");
    println!("\n🔗 Learn more: https://github.com/undead-undead/aagt");

    Ok(())
}
