// Example: Multi-Agent Trading System with Shared Namespaced Memory

use aagt_core::prelude::*;
use aagt_core::agent::{NamespacedMemory, Coordinator};
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Initialize shared memory
    let memory_manager = Arc::new(MemoryManager::new().await?);
    let shared_memory = Arc::new(NamespacedMemory::new(memory_manager.clone()));
    
    // 2. Create Coordinator
    let coordinator = Arc::new(Coordinator::new());
    
    // 3. Create specialized agents
    let provider = OpenAIProvider::new(std::env::var("OPENAI_API_KEY")?);
    
    // News Agent - monitors market news
    let news_agent = Agent::builder(provider.clone())
        .role(AgentRole::NewsMonitor)
        .with_memory(memory_manager.clone())
        .build()?;
    
    // Analyst Agent - generates trading signals
    let analyst_agent = Agent::builder(provider.clone())
        .role(AgentRole::Analyst)
        .with_memory(memory_manager.clone())
        .build()?;
    
    // Trading Agent - executes orders
    let trader_agent = Agent::builder(provider.clone())
        .role(AgentRole::Trader)
        .with_memory(memory_manager.clone())
        .build()?;
    
    // 4. Register agents
    coordinator.register("news", news_agent).await?;
    coordinator.register("analyst", analyst_agent).await?;
    coordinator.register("trader", trader_agent).await?;
    
    // 5. Simulate workflow with namespaced memory
    
    // News Agent: Store market data (5-minute TTL)
    shared_memory.store(
        "market",  // namespace
        "btc_price",
        "$43,200",
        Some(Duration::from_secs(300)),  // 5-minute TTL
        Some("CoinGeckoAPI".to_string())
    ).await?;
    
    shared_memory.store(
        "news",
        "latest",
        "Fed signals dovish stance on interest rates",
        Some(Duration::from_secs(3600)),  // 1-hour TTL
        Some("NewsAgent".to_string())
    ).await?;
    
    // Analyst Agent: Read market data and news
    if let Some(price) = shared_memory.read("market", "btc_price").await? {
        println!("Analyst: Current BTC price is {}", price);
    }
    
    if let Some(news) = shared_memory.read("news", "latest").await? {
        println!("Analyst: Latest news: {}", news);
    }
    
    // Analyst Agent: Generate signal based on data
    shared_memory.store(
        "analysis",
        "btc_signal",
        "BUY",  // Signal based on dovish Fed stance
        Some(Duration::from_secs(1800)),  // 30-minute TTL
        Some("AnalystAgent".to_string())
    ).await?;
    
    shared_memory.store(
        "analysis",
        "confidence",
        "0.75",
        Some(Duration::from_secs(1800)),
        Some("AnalystAgent".to_string())
    ).await?;
    
    // Trader Agent: Read signal and execute
    if let Some(signal) = shared_memory.read("analysis", "btc_signal").await? {
        if let Some(confidence) = shared_memory.read("analysis", "confidence").await? {
            println!("Trader: Signal={}, Confidence={}", signal, confidence);
            
            if signal == "BUY" && confidence.parse::<f64>().unwrap() > 0.7 {
                println!("Trader: Executing BUY order for BTC");
                // trader_agent.call_tool("place_order", "{ symbol: 'BTC/USDT', ..}").await?;
            }
        }
    }
    
    // 6. Demonstrate namespace isolation
    println!("\n--- Namespace Listing ---");
    
    println!("Market namespace keys:");
    for key in shared_memory.list_keys("market").await? {
        println!("  - {}", key);
    }
    
    println!("Analysis namespace keys:");
    for key in shared_memory.list_keys("analysis").await? {
        println!("  - {}", key);
    }
    
    // 7. Demonstrate metadata reading
    if let Some(entry) = shared_memory.read_with_metadata("market", "btc_price").await? {
        println!("\n--- Metadata Example ---");
        println!("Value: {}", entry.value);
        println!("Created: {}", entry.created_at);
        println!("Author: {:?}", entry.author);
        println!("Expires: {:?}", entry.expires_at);
    }
    
    println!("\n✅ Multi-agent workflow completed successfully!");
    
    Ok(())
}

/*
Expected Output:

Analyst: Current BTC price is $43,200
Analyst: Latest news: Fed signals dovish stance on interest rates
Trader: Signal=BUY, Confidence=0.75
Trader: Executing BUY order for BTC

--- Namespace Listing ---
Market namespace keys:
  - btc_price
Analysis namespace keys:
  - btc_signal
  - confidence

--- Metadata Example ---
Value: $43,200
Created: 2026-02-06T21:48:00Z
Author: Some("CoinGeckoAPI")
Expires: Some(2026-02-06T21:53:00Z)

✅ Multi-agent workflow completed successfully!
*/
