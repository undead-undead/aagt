// Example: Explicitly Enable Python Sidecar (Research Mode)

use aagt_core::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize provider
    let provider = OpenAIProvider::new(
        std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set")
    );

    // ⚠️ RESEARCH MODE: Python Sidecar enabled, DynamicSkill disabled
    let agent = Agent::builder(provider)
        .model("gpt-4o")
        .with_code_interpreter("localhost:50051").await?  // ← Explicit Sidecar
        .build()?;
    
    // The agent now has:
    // 1. Python Sidecar: ENABLED (for data analysis)
    // 2. DynamicSkill: DISABLED (auto-enable skipped due to Sidecar)
    // 
    // SECURITY WARNING: Do NOT install third-party ClawHub skills in this mode!

    println!("Agent built with Python Sidecar support!");
    
    Ok(())
}

// Output:
// (No auto-enable message - Sidecar takes precedence)
