/// Example: Markdown Memory (WriteNote Tool)
///
/// Demonstrates how to give an Agent "Explicit Memory" by allowing it to write Markdown files.
/// This acts as a persistent log or journal that is human-readable.
/// 
/// Cost Analysis:
/// - Writing a note is just a Tool Call.
/// - Token cost = Input prompt + JSON tool arguments (very small).
/// - It does NOT generate long text unless you ask it to write a novel.

use aagt_core::prelude::*;
use aagt_core::simple_tool;
use aagt_providers::gemini::{Gemini, GEMINI_2_0_FLASH};
use anyhow::Result;
use std::fs;
use std::path::Path;

// Define the WriteNote tool
simple_tool!(
    WriteNote,
    "write_note",
    "Write a structured note or log entry to a markdown file. Use this to record important decisions, daily summaries, or trading logs.",
    {
        filename: ("string", "Filename (e.g., 'daily_log.md', 'strategy_notes.md')"),
        content: ("string", "The markdown content to write")
    },
    [filename, content],
    |args| async move {
        let filename = args["filename"].as_str().unwrap();
        let content = args["content"].as_str().unwrap();
        
        // Security check: simple path traversal prevention
        if filename.contains("..") || filename.starts_with("/") {
            return Err(anyhow::anyhow!("Invalid filename. Use relative paths only."));
        }

        let path = Path::new("memory").join(filename);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Append if exists, or create new
        use tokio::io::AsyncWriteExt;
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;
            
        let entry = format!("\n\n## [{}] Log Entry\n{}\n", chrono::Local::now().to_rfc3339(), content);
        file.write_all(entry.as_bytes()).await?;

        Ok(format!("Note appended to {}", filename))
    }
);

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let provider = Gemini::from_env()?;

    let agent = Agent::builder(provider)
        .model(GEMINI_2_0_FLASH)
        .preamble(
            "You are a trading bot with explicit memory. \
            Whenever you make a decision, you MUST write it to 'trading_log.md'. \
            Keep logs concise to save tokens."
        )
        .tool(Box::new(WriteNote))
        .build()?;

    println!("🤖 Agent started. Asking for advice...");

    // This single prompt will trigger the tool call
    let response = agent.prompt(
        "Analyze the current market sentiment for Crypto (simulated data: Bitcoin $98k, Greed Index 80). \
        Should we buy or sell? Log your decision."
    ).await?;

    println!("\n🤖 Agent Response:\n{}", response);
    
    println!("\n📂 Checking Memory File (memory/trading_log.md):");
    if let Ok(content) = fs::read_to_string("memory/trading_log.md") {
        println!("{}", content);
    } else {
        println!("(No log file found - did the agent fail to write?)");
    }

    Ok(())
}
