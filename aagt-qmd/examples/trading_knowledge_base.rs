//! Example: Trading Knowledge Base
//!
//! Demonstrates using AAGT-QMD to build a trading knowledge base with:
//! - Content-addressable storage (auto-deduplication)
//! - Full-text search across strategies
//! - Fast retrieval by docid

use aagt_qmd::{Collection, QmdStore, Result};

fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    println!("🚀 AAGT-QMD Trading Knowledge Base Example\n");

    // Create store
    let mut store = QmdStore::new("trading_knowledge.db")?;
    println!("✅ Created QMD store at: trading_knowledge.db\n");

    // Create collections
    store.create_collection(Collection {
        name: "strategies".to_string(),
        description: Some("Trading strategies and analysis".to_string()),
        glob_pattern: "**/*.md".to_string(),
        root_path: None,
    })?;

    store.create_collection(Collection {
        name: "research".to_string(),
        description: Some("Market research and reports".to_string()),
        glob_pattern: "**/*.md".to_string(),
        root_path: None,
    })?;

    println!("✅ Created collections: strategies, research\n");

    // Index some documents
    println!("📝 Indexing documents...");

    let sol_doc = store.store_document(
        "strategies",
        "momentum/sol_rsi.md",
        "SOL RSI Momentum Strategy",
        r#"# SOL RSI Momentum Strategy

## Overview
Buy SOL when RSI drops below 30 (oversold), sell when RSI exceeds 70 (overbought).

## Parameters
- Asset: Solana (SOL)
- Indicator: RSI (14-period)
- Entry: RSI < 30
- Exit: RSI > 70
- Stop Loss: 5%

## Historical Performance
- Win Rate: 68%
- Average Return: 12.3%
- Sharpe Ratio: 1.8
"#,
    )?;

    let eth_doc = store.store_document(
        "strategies",
        "dip_buying/eth_support.md",
        "ETH Support Level Dip Buying",
        r#"# ETH Support Level Dip Buying

## Strategy
Buy ETH when price touches major support levels with high volume.

## Key Support Levels (2024)
- Strong: $2800, $2500
- Moderate: $3000, $2700

## Entry Conditions
1. Price touches support (±2%)
2. Volume > 1.5x average
3. No major FUD events

## Risk Management
- Position size: 10% of portfolio
- Stop loss: 3% below support
- Take profit: +15% or next resistance
"#,
    )?;

    let btc_doc = store.store_document(
        "research",
        "chainlink/btc_correlation.md",
        "Bitcoin On-Chain Metrics Analysis",
        r#"# Bitcoin On-Chain Metrics Analysis

## Key Metrics
- MVRV Ratio: Market Value to Realized Value
- NVT Ratio: Network Value to Transactions
- Miner Position Index

## Current Analysis (2024-01)
- MVRV: 2.4 (slightly bullish)
- NVT: 65 (normal range)
- Hash rate: All-time high

## Conclusion
On-chain metrics suggest accumulation phase. Long-term holders increasing positions.
"#,
    )?;

    println!("  ✓ SOL RSI Strategy (docid: #{})", sol_doc.docid);
    println!("  ✓ ETH Support Buying (docid: #{})", eth_doc.docid);
    println!("  ✓ BTC On-Chain Analysis (docid: #{})\n", btc_doc.docid);

    // Demonstrate auto-deduplication
    println!("🔄 Testing content deduplication...");
    let dup_doc = store.store_document(
        "strategies",
        "duplicate/sol_rsi_copy.md",
        "SOL RSI Strategy (Copy)",
        r#"# SOL RSI Momentum Strategy

## Overview
Buy SOL when RSI drops below 30 (oversold), sell when RSI exceeds 70 (overbought).

## Parameters
- Asset: Solana (SOL)
- Indicator: RSI (14-period)
- Entry: RSI < 30
- Exit: RSI > 70
- Stop Loss: 5%

## Historical Performance
- Win Rate: 68%
- Average Return: 12.3%
- Sharpe Ratio: 1.8
"#,
    )?;

    if sol_doc.hash == dup_doc.hash {
        println!("  ✓ Same content detected! Hash: {}", &sol_doc.hash[..12]);
        println!("  ✓ Storage space saved via deduplication\n");
    }

    // Full-text search
    println!("🔍 Full-Text Search Examples:\n");

    println!("Query: 'RSI trading'");
    let rsi_results = store.search_fts("RSI trading", 5)?;
    for (i, result) in rsi_results.iter().enumerate() {
        println!(
            "  {}. {} (score: {:.2})",
            i + 1,
            result.document.title,
            result.score
        );
        if let Some(snippet) = &result.snippet {
            println!("     {}\n", snippet);
        }
    }

    println!("Query: 'support levels'");
    let support_results = store.search_fts("support levels", 5)?;
    for (i, result) in support_results.iter().enumerate() {
        println!(
            "  {}. {} (score: {:.2})",
            i + 1,
            result.document.title,
            result.score
        );
    }
    println!();

    // Collection-specific search
    println!("Query: 'strategy' (strategies collection only)");
    let strategy_results = store.search_fts_in_collection("strategy", "strategies", 5)?;
    for (i, result) in strategy_results.iter().enumerate() {
        println!("  {}. {}", i + 1, result.document.title);
    }
    println!();

    // Retrieve by docid
    println!("📖 Fast Retrieval by docid:\n");
    if let Some(doc) = store.get_by_docid(&sol_doc.docid)? {
        println!("  Docid: #{}", doc.docid);
        println!("  Path: {}/{}", doc.collection, doc.path);
        println!("  Title: {}", doc.title);
        println!("  Hash: {}", &doc.hash[..16]);
    }
    println!();

    // Stats
    println!("📊 Store Statistics:\n");
    let stats = store.get_stats()?;
    println!("  Total Documents: {}", stats.total_documents);
    println!("  Total Collections: {}", stats.total_collections);
    println!("  Unique Content Blocks: {}", stats.total_unique_content);
    println!(
        "  Database Size: {:.2} KB",
        stats.database_size_bytes as f64 / 1024.0
    );
    println!(
        "\n  Deduplication Ratio: {:.1}%",
        (1.0 - stats.total_unique_content as f64 / stats.total_documents as f64) * 100.0
    );

    println!("\n✅ Example completed successfully!");
    println!("   Database saved at: trading_knowledge.db");

    Ok(())
}
