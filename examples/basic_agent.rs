/// Basic example of creating and using an AAGT agent
/// 
/// This example demonstrates:
/// - Creating a Gemini provider
/// - Building an agent with custom configuration
/// - Simple prompt-response interaction

use aagt_core::prelude::*;
use aagt_providers::gemini::{Gemini, GEMINI_2_0_FLASH};
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing for logs
    tracing_subscriber::fmt::init();

    // 1. Create provider (requires GEMINI_API_KEY env var)
    let provider = Gemini::from_env()?;

    // 2. Build agent with configuration
    let agent = Agent::builder(provider)
        .model(GEMINI_2_0_FLASH)
        .preamble("You are a helpful AI assistant.")
        .temperature(0.7)
        .max_tokens(1000)
        .build()?;

    // 3. Simple interaction
    println!("Agent: Hello! I'm ready to help.");
    
    let response = agent.prompt("What is Rust and why is it good for AI agents?").await?;
    println!("\nAgent: {}", response);

    Ok(())
}
