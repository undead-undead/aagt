//! Comprehensive test for all AAGT providers
//!
//! This example tests all 8 supported LLM providers and shows their capabilities.
//!
//! Run with: cargo run --example test_all_providers --features full
//!
//! Required environment variables (set only the ones you have):
//! - OPENAI_API_KEY
//! - ANTHROPIC_API_KEY  
//! - GEMINI_API_KEY
//! - DEEPSEEK_API_KEY
//! - MOONSHOT_API_KEY
//! - OPENROUTER_API_KEY
//! - GROQ_API_KEY
//! - OLLAMA_BASE_URL (optional, defaults to http://localhost:11434/v1)

use aagt_core::prelude::*;
use std::time::Instant;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .init();

    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║                                                              ║");
    println!("║       🧪 AAGT ALL PROVIDERS COMPREHENSIVE TEST               ║");
    println!("║                                                              ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    let test_prompt = "What is 2+2? Answer in one word.";
    let system_prompt = "You are a helpful assistant.";
    
    let mut success_count = 0;
    let mut total_tests = 0;

    // Test 1: OpenAI
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("1️⃣  OpenAI (GPT-4o-mini)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    total_tests += 1;
    #[cfg(feature = "openai")]
    {
        use aagt_providers::openai::{OpenAI, GPT_4O_MINI};
        match test_provider(
            "OpenAI",
            OpenAI::from_env(),
            GPT_4O_MINI,
            system_prompt,
            test_prompt,
        ).await {
            Ok(time) => {
                println!("✅ Success! Response time: {:.2}s", time);
                success_count += 1;
            }
            Err(e) => println!("❌ Failed: {}", e),
        }
    }
    #[cfg(not(feature = "openai"))]
    println!("⚠️  Skipped (feature not enabled)");
    println!();

    // Test 2: Anthropic
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("2️⃣  Anthropic (Claude)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    total_tests += 1;
    #[cfg(feature = "anthropic")]
    {
        use aagt_providers::anthropic::Anthropic;
        match test_provider(
            "Anthropic",
            Anthropic::from_env(),
            "claude-3-5-haiku-20241022",
            system_prompt,
            test_prompt,
        ).await {
            Ok(time) => {
                println!("✅ Success! Response time: {:.2}s", time);
                success_count += 1;
            }
            Err(e) => println!("❌ Failed: {}", e),
        }
    }
    #[cfg(not(feature = "anthropic"))]
    println!("⚠️  Skipped (feature not enabled)");
    println!();

    // Test 3: Gemini
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("3️⃣  Google Gemini");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    total_tests += 1;
    #[cfg(feature = "gemini")]
    {
        use aagt_providers::gemini::Gemini;
        match test_provider(
            "Gemini",
            Gemini::from_env(),
            "gemini-2.0-flash-exp",
            system_prompt,
            test_prompt,
        ).await {
            Ok(time) => {
                println!("✅ Success! Response time: {:.2}s", time);
                success_count += 1;
            }
            Err(e) => println!("❌ Failed: {}", e),
        }
    }
    #[cfg(not(feature = "gemini"))]
    println!("⚠️  Skipped (feature not enabled)");
    println!();

    // Test 4: DeepSeek
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("4️⃣  DeepSeek 🇨🇳 (Cost-Effective)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    total_tests += 1;
    #[cfg(feature = "deepseek")]
    {
        use aagt_providers::deepseek::{DeepSeek, DEEPSEEK_CHAT};
        match test_provider(
            "DeepSeek",
            DeepSeek::from_env(),
            DEEPSEEK_CHAT,
            system_prompt,
            test_prompt,
        ).await {
            Ok(time) => {
                println!("✅ Success! Response time: {:.2}s", time);
                success_count += 1;
            }
            Err(e) => println!("❌ Failed: {}", e),
        }
    }
    #[cfg(not(feature = "deepseek"))]
    println!("⚠️  Skipped (feature not enabled)");
    println!();

    // Test 5: Moonshot (Kimi)
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("5️⃣  Moonshot 🇨🇳 (Kimi - Long Context)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    total_tests += 1;
    #[cfg(feature = "moonshot")]
    {
        use aagt_providers::moonshot::{Moonshot, MOONSHOT_V1_8K};
        match test_provider(
            "Moonshot",
            Moonshot::from_env(),
            MOONSHOT_V1_8K,
            system_prompt,
            test_prompt,
        ).await {
            Ok(time) => {
                println!("✅ Success! Response time: {:.2}s", time);
                success_count += 1;
            }
            Err(e) => println!("❌ Failed: {}", e),
        }
    }
    #[cfg(not(feature = "moonshot"))]
    println!("⚠️  Skipped (feature not enabled)");
    println!();

    // Test 6: OpenRouter
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("6️⃣  OpenRouter (Multi-Model Gateway)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    total_tests += 1;
    #[cfg(feature = "openrouter")]
    {
        use aagt_providers::openrouter::OpenRouter;
        match test_provider(
            "OpenRouter",
            OpenRouter::from_env(),
            "meta-llama/llama-3.2-3b-instruct:free",
            system_prompt,
            test_prompt,
        ).await {
            Ok(time) => {
                println!("✅ Success! Response time: {:.2}s", time);
                success_count += 1;
            }
            Err(e) => println!("❌ Failed: {}", e),
        }
    }
    #[cfg(not(feature = "openrouter"))]
    println!("⚠️  Skipped (feature not enabled)");
    println!();

    // Test 7: Groq (NEW!)
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("7️⃣  Groq ⚡ (Ultra-Fast - NEW!)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    total_tests += 1;
    #[cfg(feature = "groq")]
    {
        use aagt_providers::groq::{Groq, LLAMA_3_1_8B};
        match test_provider(
            "Groq",
            Groq::from_env(),
            LLAMA_3_1_8B,
            system_prompt,
            test_prompt,
        ).await {
            Ok(time) => {
                println!("✅ Success! Response time: {:.2}s 🚀 (Speed King!)", time);
                success_count += 1;
            }
            Err(e) => println!("❌ Failed: {}", e),
        }
    }
    #[cfg(not(feature = "groq"))]
    println!("⚠️  Skipped (feature not enabled)");
    println!();

    // Test 8: Ollama (NEW!)
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("8️⃣  Ollama 🔐 (Local & Private - NEW!)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    total_tests += 1;
    #[cfg(feature = "ollama")]
    {
        use aagt_providers::ollama::{Ollama, LLAMA_3_1_8B};
        match test_provider(
            "Ollama",
            Ollama::from_env(),
            LLAMA_3_1_8B,
            system_prompt,
            test_prompt,
        ).await {
            Ok(time) => {
                println!("✅ Success! Response time: {:.2}s 🔐 (Privacy King!)", time);
                success_count += 1;
            }
            Err(e) => println!("❌ Failed: {} (Is Ollama running?)", e),
        }
    }
    #[cfg(not(feature = "ollama"))]
    println!("⚠️  Skipped (feature not enabled)");
    println!();

    // Final Summary
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║                    📊 TEST SUMMARY                           ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();
    println!("Total Providers: {}", total_tests);
    println!("✅ Successful: {}", success_count);
    println!("❌ Failed: {}", total_tests - success_count);
    println!("Success Rate: {:.1}%", (success_count as f64 / total_tests as f64) * 100.0);
    println!();

    if success_count > 0 {
        println!("🎉 At least one provider is working!");
        println!();
        println!("💡 Tips:");
        println!("   • Set more API keys to test other providers");
        println!("   • For Ollama: Install and run 'ollama serve'");
        println!("   • Check GROQ_OLLAMA_GUIDE.md for setup instructions");
    } else {
        println!("⚠️  No providers succeeded.");
        println!();
        println!("💡 Setup Instructions:");
        println!("   1. Set at least one API key (e.g., export OPENAI_API_KEY=...)");
        println!("   2. Or install Ollama for local testing");
        println!("   3. See GROQ_OLLAMA_GUIDE.md for details");
    }
    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    Ok(())
}

async fn test_provider<P: Provider>(
    name: &str,
    provider_result: Result<P>,
    model: &str,
    system_prompt: &str,
    prompt: &str,
) -> Result<f64> {
    let provider = provider_result?;
    
    println!("Provider: {}", name);
    println!("Model: {}", model);
    println!("Testing...");
    
    let agent = Agent::builder(provider)
        .model(model)
        .system_prompt(system_prompt)
        .build()?;

    let start = Instant::now();
    let response = agent.prompt(prompt).await?;
    let elapsed = start.elapsed().as_secs_f64();

    println!("Response: {}", response.trim());
    
    Ok(elapsed)
}
