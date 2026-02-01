/// Production Tracing Configuration
/// 
/// This example shows how to configure tracing for production:
/// - Async file writing (low memory)
/// - JSON format (machine-readable)
/// - Daily log rotation
/// - Environment-based filtering

use aagt_core::prelude::*;
use aagt_providers::gemini::{Gemini, GEMINI_2_0_FLASH};
use anyhow::Result;
use tracing::info;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{fmt, EnvFilter};

fn init_production_tracing() {
    // Create rolling file appender (rotates daily)
    let file_appender = RollingFileAppender::new(
        Rotation::DAILY,
        "./logs",      // Log directory
        "aagt.log"     // File prefix (creates aagt.log.2026-02-01, etc.)
    );

    // Configure subscriber
    fmt()
        .with_writer(file_appender)
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive("aagt=info".parse().unwrap())      // AAGT logs at INFO
                .add_directive("aagt_core=info".parse().unwrap()) // Core logs at INFO
                .add_directive("hyper=warn".parse().unwrap())     // HTTP client at WARN
                .add_directive("reqwest=warn".parse().unwrap())   // Reqwest at WARN
        )
        .with_ansi(false)  // No color codes in files
        .json()            // JSON format for log aggregation
        .init();
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize production tracing
    init_production_tracing();

    info!(
        service = "aagt-agent",
        version = env!("CARGO_PKG_VERSION"),
        "Service started"
    );

    // Create agent
    let provider = Gemini::from_env()?;
    let agent = Agent::builder(provider)
        .model(GEMINI_2_0_FLASH)
        .preamble("You are a trading assistant.")
        .build()?;

    // Simulate production workload
    for i in 1..=5 {
        info!(iteration = i, "Processing request");
        
        let response = agent
            .prompt(format!("Analyze market trend #{}", i))
            .await?;
        
        info!(
            iteration = i,
            response_len = response.len(),
            "Request completed"
        );
    }

    info!("Service shutting down");

    println!("\n✅ Logs written to ./logs/aagt.log.YYYY-MM-DD");
    println!("📊 View logs: tail -f ./logs/aagt.log.*");
    println!("🔍 Parse JSON: cat ./logs/aagt.log.* | jq .");

    Ok(())
}
