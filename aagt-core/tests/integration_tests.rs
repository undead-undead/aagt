//! Integration tests for aagt-core

use aagt_core::prelude::*;

#[test]
fn test_message_creation() {
    let user_msg = Message::user("Hello");
    assert_eq!(user_msg.role, Role::User);
    assert_eq!(user_msg.content.as_text(), "Hello");

    let assistant_msg = Message::assistant("Hi there!");
    assert_eq!(assistant_msg.role, Role::Assistant);

    let system_msg = Message::system("You are helpful");
    assert_eq!(system_msg.role, Role::System);
}

#[test]
fn test_tool_definition() {
    let def = ToolDefinition {
        name: "get_price".to_string(),
        description: "Get token price".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "symbol": {"type": "string"}
            }
        }),
    };

    assert_eq!(def.name, "get_price");
}

#[test]
fn test_toolset_basic() {
    use aagt_core::tool::ToolSet;

    let toolset = ToolSet::new();
    assert!(toolset.is_empty());
    assert_eq!(toolset.len(), 0);
}

#[test]
fn test_agent_config_default() {
    use aagt_core::agent::AgentConfig;

    let config = AgentConfig::default();
    assert_eq!(config.model, "gpt-4o");
    assert_eq!(config.preamble, "You are a helpful AI assistant.");
    assert_eq!(config.max_tokens, Some(4096));
    assert_eq!(config.temperature, Some(0.7));
}

#[tokio::test]
async fn test_memory_short_term() {
    use aagt_core::memory::{Memory, ShortTermMemory};

    let memory = ShortTermMemory::new(5, 100);
    
    memory.store("user1", None, Message::user("Message 1")).await.unwrap();
    memory.store("user1", None, Message::user("Message 2")).await.unwrap();
    memory.store("user1", None, Message::user("Message 3")).await.unwrap();
    
    assert_eq!(memory.message_count("user1", None), 3);
    
    // Test capacity limits
    memory.store("user1", None, Message::user("Message 4")).await.unwrap();
    memory.store("user1", None, Message::user("Message 5")).await.unwrap();
    memory.store("user1", None, Message::user("Message 6")).await.unwrap();
    
    assert_eq!(memory.message_count("user1", None), 5); // Should be capped
    
    // Test retrieve
    let messages = memory.retrieve("user1", None, 3).await;
    assert_eq!(messages.len(), 3);
}

#[tokio::test]
async fn test_memory_long_term() {
    use aagt_core::memory::{LongTermMemory, MemoryEntry};
    let path = std::path::PathBuf::from("test_integration.jsonl");
    if path.exists() {
        let _ = std::fs::remove_file(&path);
    }
    let memory = LongTermMemory::new(100, path).await.unwrap();
    
    let entry = MemoryEntry {
        id: "test-1".to_string(),
        user_id: "user1".to_string(),
        content: "User prefers SOL".to_string(),
        timestamp: chrono::Utc::now().timestamp(),
        tags: vec!["crypto".to_string()],
        relevance: 1.0,
    };
    
    memory.store_entry(entry, None).await.unwrap();
    
    let retrieved = memory.retrieve_by_tag("user1", "crypto", None, 10).await;
    assert_eq!(retrieved.len(), 1);
    assert_eq!(retrieved[0].content, "User prefers SOL");
}

#[test]
fn test_risk_config() {
    use aagt_core::risk::RiskConfig;

    let config = RiskConfig::default();
    assert!(config.max_single_trade_usd > 0.0);
    assert!(config.max_daily_volume_usd > 0.0);
}

#[tokio::test]
async fn test_risk_manager_basic_checks() {
    use aagt_core::risk::{RiskManager, RiskConfig, TradeContext, InMemoryRiskStore};
    use std::sync::Arc;

    let config = RiskConfig {
        max_single_trade_usd: 10_000.0,
        max_daily_volume_usd: 50_000.0,
        max_slippage_percent: 5.0,
        min_liquidity_usd: 100_000.0,
        enable_rug_detection: true,
        trade_cooldown_secs: 5,
    };

    let manager = RiskManager::with_config(config, Arc::new(InMemoryRiskStore)).await.unwrap();

    // Test a valid trade
    let valid_trade = TradeContext {
        user_id: "user1".to_string(),
        from_token: "USDC".to_string(),
        to_token: "SOL".to_string(),
        amount_usd: 1000.0,
        expected_slippage: 0.5,
        liquidity_usd: Some(500_000.0),
        is_flagged: false,
    };

    assert!(manager.check_and_reserve(&valid_trade).await.is_ok());

    // Test trade exceeding limit
    let large_trade = TradeContext {
        user_id: "user1".to_string(),
        from_token: "USDC".to_string(),
        to_token: "SOL".to_string(),
        amount_usd: 15_000.0, // Exceeds 10k limit
        expected_slippage: 0.5,
        liquidity_usd: Some(500_000.0),
        is_flagged: false,
    };

    assert!(manager.check_and_reserve(&large_trade).await.is_err());
}

#[test]
fn test_strategy_condition_serialization() {
    use aagt_core::strategy::Condition;

    let condition = Condition::PriceAbove {
        token: "SOL".to_string(),
        threshold: 200.0,
    };

    let json = serde_json::to_string(&condition).unwrap();
    let parsed: Condition = serde_json::from_str(&json).unwrap();
    
    match parsed {
        Condition::PriceAbove { token, threshold } => {
            assert_eq!(token, "SOL");
            assert_eq!(threshold, 200.0);
        }
        _ => panic!("Wrong condition type"),
    }
}

#[tokio::test]
async fn test_simulation_basic() {
    use aagt_core::simulation::{BasicSimulator, SimulationRequest, Simulator};

    let simulator = BasicSimulator::new();
    
    let request = SimulationRequest {
        from_token: "USDC".to_string(),
        to_token: "SOL".to_string(),
        amount: 1000.0,
        slippage_tolerance: 1.0,
        chain: "solana".to_string(),
        exchange: None,
    };

    let result = simulator.simulate(&request).await.unwrap();
    
    assert!(result.success);
    assert!(result.output_amount > 0.0);
    assert!(result.gas_cost_usd >= 0.0);
}

#[test]
fn test_error_types() {
    use aagt_core::error::Error;

    let err = Error::agent_config("Invalid model");
    assert!(matches!(err, Error::AgentConfig { .. }));

    let err2 = Error::tool_execution("my_tool", "failed to run");
    assert!(matches!(err2, Error::ToolExecution { .. }));
}

#[test]
fn test_streaming_choice_types() {
    use aagt_core::streaming::StreamingChoice;

    let msg = StreamingChoice::Message("Hello".to_string());
    assert!(matches!(msg, StreamingChoice::Message(_)));

    let tool = StreamingChoice::ToolCall {
        id: "call_1".to_string(),
        name: "get_price".to_string(),
        arguments: serde_json::json!({"symbol": "SOL"}),
    };
    
    if let StreamingChoice::ToolCall { name, .. } = tool {
        assert_eq!(name, "get_price");
    }

    let done = StreamingChoice::Done;
    assert!(matches!(done, StreamingChoice::Done));
}

#[test]
fn test_message_builder() {
    let msg = Message::user("Hello")
        .with_name("Alice");
    
    assert_eq!(msg.name, Some("Alice".to_string()));
}

#[test]
fn test_tool_call_creation() {
    let call = ToolCall::new("call_123", "get_price", serde_json::json!({"symbol": "SOL"}));
    
    assert_eq!(call.id, "call_123");
    assert_eq!(call.name, "get_price");
}

#[tokio::test]
async fn test_memory_manager() {
    let manager = MemoryManager::new().await.unwrap();
    
    // Store a message
    manager.short_term.store("user1", None, Message::user("Hello")).await.unwrap();
    
    // Retrieve
    let messages = manager.short_term.retrieve("user1", None, 10).await;
    assert_eq!(messages.len(), 1);
}
