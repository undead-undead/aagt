use aagt_core::risk::*;
use chrono::{DateTime, Duration, Utc};
use std::sync::Arc;
use std::collections::HashMap;

#[tokio::test]
async fn verify_risk_daily_reset_preserves_pending() {
    // 1. Setup RiskManager with a mock store
    let config = RiskConfig {
        max_daily_volume_usd: 1000.0,
        min_liquidity_usd: 1000.0, // Match test data (which is 10000.0)
        ..Default::default()
    };
    
    // We need to manually manipulate state to simulate day change, 
    // but RiskManager encapsulates state.
    // However, we can use the `RiskStateStore` to inject state or 
    // rely on the daily reset logic inside `check_and_reserve`.
    
    // Trick: We can impl a MockStore that returns a state from "Yesterday"
    
    struct MockStore {
        initial_state: HashMap<String, UserState>
    }
    
    #[async_trait::async_trait]
    impl RiskStateStore for MockStore {
        async fn load(&self) -> aagt_core::error::Result<HashMap<String, UserState>> {
            Ok(self.initial_state.clone())
        }
        async fn save(&self, _: &HashMap<String, UserState>) -> aagt_core::error::Result<()> {
            Ok(())
        }
    }
    
    // Create state from yesterday
    let mut state = HashMap::new();
    let yesterday = Utc::now() - Duration::days(1);
    
    state.insert("user1".to_string(), UserState {
        daily_volume_usd: 500.0, // Should be cleared
        pending_volume_usd: 100.0, // Should NOT be cleared
        last_trade: Some(yesterday),
        volume_reset: yesterday,
    });
    
    let store = Arc::new(MockStore { initial_state: state });
    let manager = RiskManager::with_config(config, store).await.unwrap();
    
    // 2. Perform a check. This triggers `handle_check_and_reserve`.
    // The internal logic checks date, sees it's new day, and resets.
    
    let context = TradeContext {
        user_id: "user1".to_string(),
        from_token: "USDC".to_string(),
        to_token: "SOL".to_string(),
        amount_usd: 50.0,
        expected_slippage: 0.1,
        liquidity_usd: Some(10000.0),
        is_flagged: false,
    };
    
    // This check should pass
    // AND it should have triggered a reset internally.
    manager.check_and_reserve(&context).await.unwrap();
    
    // 3. Verify state via `remaining_daily_limit`
    // Limit = 1000.
    // Daily Volume should have been reset to 0 (previously 500).
    // Pending Volume should be preserved (100) + New Pending (50) = 150.
    // Remaining = 1000 - (0 + 150) = 850.
    
    // If bug existed (pending cleared):
    // Daily = 0.
    // Pending = 50.
    // Remaining = 950.
    
    let remaining = manager.remaining_daily_limit("user1").await;
    println!("Remaining Limit: {}", remaining);
    
    assert_eq!(remaining, 850.0, "Pending volume incorrectly cleared on daily reset!");
}
