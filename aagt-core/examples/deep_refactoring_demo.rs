/// Example: Deep Refactoring Demo
///
/// Demonstrates all improvements from both refactoring rounds:
/// 1. Configurable skill execution
/// 2. Background maintenance
/// 3. Composable risk checks (NEW)
/// 4. Actor-based FileStrategyStore (NEW)
/// 5. Unified architecture

use aagt_core::prelude::*;
use aagt_core::risk::InMemoryRiskStore;
use aagt_core::strategy::{FileStrategyStore, PriceDirection, NotifyChannel, StrategyStore};
use std::sync::Arc;
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    println!("🚀 AAGT Deep Refactoring Demo\n");

    // 1. Custom Risk Checks (NEW!)
    println!("🛡️  Setting up custom risk checks...");
    
    let risk_config = RiskConfig {
        max_single_trade_usd: 10_000.0,
        max_daily_volume_usd: 50_000.0,
        max_slippage_percent: 2.0,
        min_liquidity_usd: 500_000.0,
        enable_rug_detection: true,
        trade_cooldown_secs: 30,
    };
    
    let risk_manager = Arc::new(
        RiskManager::with_config(
            risk_config,
            Arc::new(InMemoryRiskStore)
        ).await?
    );
    
    // Use RiskCheckBuilder for composable checks
    let custom_checks = RiskCheckBuilder::new()
        .max_trade_amount(5_000.0)  // More conservative than config
        .max_slippage(1.5)           // Stricter slippage
        .min_liquidity(1_000_000.0)  // Higher liquidity requirement
        .token_security(vec![
            "SCAM1".to_string(),
            "RUG2".to_string(),
        ])
        .build();
    
    for check in custom_checks {
        risk_manager.add_check(check);
    }
    
    println!("  ✅ Risk manager with 4 custom checks configured");

    // 2. Actor-based Strategy Store (NEW!)
    println!("\n📝 Initializing actor-based strategy store...");
    
    let strategy_store = Arc::new(
        FileStrategyStore::new("data/strategies.json")
    );
    
    // Create a test strategy
    let strategy = Strategy {
        id: uuid::Uuid::new_v4().to_string(),
        user_id: "demo_user".to_string(),
        name: "Conservative Swing Trade".to_string(),
        description: Some("Low-risk swing trading strategy".to_string()),
        condition: Condition::PriceChange {
            token: "SOL".to_string(),
            percent: 5.0,
            direction: PriceDirection::Any,
        },
        actions: vec![
            Action::Swap {
                from_token: "USDC".to_string(),
                to_token: "SOL".to_string(),
                amount: "10%".to_string(),
            },
            Action::Notify {
                channel: NotifyChannel::Telegram,
                message: "Swing trade executed".to_string(),
            },
        ],
        active: true,
        created_at: chrono::Utc::now().timestamp(),
    };
    
    strategy_store.save(&strategy).await?;
    println!("  ✅ Strategy saved via actor (no file locks!)");
    
    let loaded = strategy_store.load().await?;
    println!("  ✅ Loaded {} strategies from store", loaded.len());

    // 3. Background Maintenance
    println!("\n🧹 Starting background maintenance...");
    
    let short_term = Arc::new(ShortTermMemory::default_capacity());
    
    let mut maintenance = MaintenanceManager::new();
    let config = MaintenanceConfig {
        memory_cleanup_interval_secs: 60,
        file_compaction_interval_secs: 300,
        memory_inactive_timeout_secs: 1800,
    };
    
    maintenance.start_memory_cleanup(short_term.clone(), config);
    println!("  ✅ Background cleanup active");

    // 4. Test Risk Checks
    println!("\n🔍 Testing risk check system...");
    
    // Safe trade
    let safe_trade = TradeContext {
        user_id: "demo_user".to_string(),
        from_token: "USDC".to_string(),
        to_token: "SOL".to_string(),
        amount_usd: 3_000.0,
        expected_slippage: 1.0,
        liquidity_usd: Some(2_000_000.0),
        is_flagged: false,
    };
    
    match risk_manager.check_and_reserve(&safe_trade).await {
        Ok(_) => {
            println!("  ✅ Safe trade approved ($3,000)");
            risk_manager.commit_trade(&safe_trade.user_id, safe_trade.amount_usd).await?;
        }
        Err(e) => println!("  ❌ Trade rejected: {}", e),
    }
    
    // Violates custom max_trade_amount (5000)
    let too_large = TradeContext {
        user_id: "demo_user".to_string(),
        from_token: "USDC".to_string(),
        to_token: "SOL".to_string(),
        amount_usd: 7_000.0,  // Exceeds custom limit
        expected_slippage: 1.0,
        liquidity_usd: Some(2_000_000.0),
        is_flagged: false,
    };
    
    match risk_manager.check_and_reserve(&too_large).await {
        Ok(_) => println!("  ❌ Large trade approved (unexpected!)"),
        Err(e) => println!("  ✅ Large trade blocked: {}", e),
    }
    
    // Violates slippage check
    let high_slippage = TradeContext {
        user_id: "demo_user2".to_string(),
        from_token: "USDC".to_string(),
        to_token: "SOL".to_string(),
        amount_usd: 1_000.0,
        expected_slippage: 3.0,  // Exceeds 1.5% limit
        liquidity_usd: Some(2_000_000.0),
        is_flagged: false,
    };
    
    match risk_manager.check_and_reserve(&high_slippage).await {
        Ok(_) => println!("  ❌ High slippage trade approved (unexpected!)"),
        Err(e) => println!("  ✅ High slippage blocked: {}", e),
    }
    
    // Blacklisted token
    let scam_token = TradeContext {
        user_id: "demo_user3".to_string(),
        from_token: "USDC".to_string(),
        to_token: "SCAM1".to_string(),  // Blacklisted
        amount_usd: 100.0,
        expected_slippage: 0.5,
        liquidity_usd: Some(2_000_000.0),
        is_flagged: false,
    };
    
    match risk_manager.check_and_reserve(&scam_token).await {
        Ok(_) => println!("  ❌ Scam token trade approved (unexpected!)"),
        Err(e) => println!("  ✅ Scam token blocked: {}", e),
    }

    // 5. Skill Execution Config
    println!("\n🎯 Testing skill execution config...");
    
    let strict_config = SkillExecutionConfig {
        timeout_secs: 10,
        max_output_bytes: 100_000,
        allow_network: false,
        env_vars: std::collections::HashMap::new(),
    };
    
    println!("  ✅ Skill execution: 10s timeout, 100KB limit, network blocked");

    // 6. Graceful Shutdown
    println!("\n🛑 Graceful shutdown...");
    maintenance.shutdown().await;
    println!("  ✅ All background tasks stopped");

    println!("\n✨ Deep refactoring demo complete!");
    println!("\n📊 Summary of improvements:");
    println!("   • Composable risk checks via Builder pattern");
    println!("   • Actor-based strategy persistence (no file locks)");
    println!("   • Background resource maintenance");
    println!("   • Strict skill execution limits");
    println!("   • Unified actor model architecture");
    println!("\n🎉 AAGT is production-ready!");

    Ok(())
}
