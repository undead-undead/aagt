//! Risk control system for trading operations
//!
//! Provides safety checks before executing trades.

use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Risk check result
#[derive(Debug, Clone)]
pub enum RiskCheckResult {
    /// Check passed
    Approved,
    /// Check failed with reason
    Rejected { reason: String },
    /// Needs manual review
    PendingReview { reason: String },
}

impl RiskCheckResult {
    /// Check if approved
    pub fn is_approved(&self) -> bool {
        matches!(self, Self::Approved)
    }
}

/// Configuration for risk controls
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskConfig {
    /// Maximum single trade amount in USD
    pub max_single_trade_usd: f64,
    /// Maximum daily trade volume in USD
    pub max_daily_volume_usd: f64,
    /// Maximum slippage percentage allowed
    pub max_slippage_percent: f64,
    /// Minimum liquidity required in USD
    pub min_liquidity_usd: f64,
    /// Enable rug pull detection
    pub enable_rug_detection: bool,
    /// Cooldown between trades in seconds
    pub trade_cooldown_secs: u64,
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            max_single_trade_usd: 10_000.0,
            max_daily_volume_usd: 50_000.0,
            max_slippage_percent: 5.0,
            min_liquidity_usd: 100_000.0,
            enable_rug_detection: true,
            trade_cooldown_secs: 5,
        }
    }
}

/// A risk check that can be performed
pub trait RiskCheck: Send + Sync {
    /// Name of this check
    fn name(&self) -> &str;

    /// Perform the check
    fn check(&self, context: &TradeContext) -> RiskCheckResult;
}

/// Context for a trade being checked
#[derive(Debug, Clone)]
pub struct TradeContext {
    /// User ID
    pub user_id: String,
    /// Token being sold
    pub from_token: String,
    /// Token being bought
    pub to_token: String,
    /// Amount in USD
    pub amount_usd: f64,
    /// Expected slippage
    pub expected_slippage: f64,
    /// Token liquidity in USD
    pub liquidity_usd: Option<f64>,
    /// Is this token flagged as risky
    pub is_flagged: bool,
}

/// User trading state for tracking limits
struct UserState {
    /// Daily volume traded
    daily_volume_usd: f64,
    /// Last trade timestamp
    last_trade: Option<Instant>,
    /// Volume reset time
    volume_reset: Instant,
}

impl Default for UserState {
    fn default() -> Self {
        Self {
            daily_volume_usd: 0.0,
            last_trade: None,
            volume_reset: Instant::now(),
        }
    }
}

/// The main risk manager
pub struct RiskManager {
    config: RiskConfig,
    /// Per-user state tracking
    user_states: DashMap<String, UserState>,
    /// Custom checks
    custom_checks: Vec<Arc<dyn RiskCheck>>,
}

impl RiskManager {
    /// Create with default config
    pub fn new() -> Self {
        Self::with_config(RiskConfig::default())
    }

    /// Create with custom config
    pub fn with_config(config: RiskConfig) -> Self {
        Self {
            config,
            user_states: DashMap::new(),
            custom_checks: Vec::new(),
        }
    }

    /// Add a custom risk check
    pub fn add_check(&mut self, check: Arc<dyn RiskCheck>) {
        self.custom_checks.push(check);
    }

    /// Perform all risk checks for a trade
    pub fn check_trade(&self, context: &TradeContext) -> Result<()> {
        // 1. Single trade limit
        if context.amount_usd > self.config.max_single_trade_usd {
            return Err(Error::RiskLimitExceeded {
                limit_type: "single_trade".to_string(),
                current: format!("${:.2}", context.amount_usd),
                max: format!("${:.2}", self.config.max_single_trade_usd),
            });
        }

        // 2. Daily volume check
        let mut state = self.user_states.entry(context.user_id.clone()).or_default();

        // Reset daily volume if needed (24 hour window)
        if state.volume_reset.elapsed() > Duration::from_secs(86400) {
            state.daily_volume_usd = 0.0;
            state.volume_reset = Instant::now();
        }

        let projected_daily = state.daily_volume_usd + context.amount_usd;
        if projected_daily > self.config.max_daily_volume_usd {
            return Err(Error::RiskLimitExceeded {
                limit_type: "daily_volume".to_string(),
                current: format!("${:.2}", projected_daily),
                max: format!("${:.2}", self.config.max_daily_volume_usd),
            });
        }

        // 3. Trade cooldown
        if let Some(last) = state.last_trade {
            let cooldown = Duration::from_secs(self.config.trade_cooldown_secs);
            if last.elapsed() < cooldown {
                let wait = cooldown - last.elapsed();
                return Err(Error::risk_check_failed(
                    "cooldown",
                    format!("Please wait {} seconds between trades", wait.as_secs()),
                ));
            }
        }

        // 4. Slippage check
        if context.expected_slippage > self.config.max_slippage_percent {
            return Err(Error::risk_check_failed(
                "slippage",
                format!(
                    "Slippage {:.2}% exceeds maximum {:.2}%",
                    context.expected_slippage, self.config.max_slippage_percent
                ),
            ));
        }

        // 5. Liquidity check
        if let Some(liquidity) = context.liquidity_usd {
            if liquidity < self.config.min_liquidity_usd {
                return Err(Error::risk_check_failed(
                    "liquidity",
                    format!(
                        "Token liquidity ${:.0} below minimum ${:.0}",
                        liquidity, self.config.min_liquidity_usd
                    ),
                ));
            }
        }

        // 6. Rug pull detection
        if self.config.enable_rug_detection && context.is_flagged {
            return Err(Error::risk_check_failed(
                "rug_detection",
                "Token is flagged as potentially risky",
            ));
        }

        // 7. Run custom checks
        for check in &self.custom_checks {
            match check.check(context) {
                RiskCheckResult::Approved => {}
                RiskCheckResult::Rejected { reason } => {
                    return Err(Error::RiskCheckFailed {
                        check_name: check.name().to_string(),
                        reason,
                    });
                }
                RiskCheckResult::PendingReview { reason } => {
                    return Err(Error::risk_check_failed(check.name(), reason));
                }
            }
        }

        Ok(())
    }

    /// Record a completed trade (updates user state)
    pub fn record_trade(&self, user_id: &str, amount_usd: f64) {
        let mut state = self.user_states.entry(user_id.to_string()).or_default();
        state.daily_volume_usd += amount_usd;
        state.last_trade = Some(Instant::now());
    }

    /// Get remaining daily limit for a user
    pub fn remaining_daily_limit(&self, user_id: &str) -> f64 {
        self.user_states
            .get(user_id)
            .map(|s| self.config.max_daily_volume_usd - s.daily_volume_usd)
            .unwrap_or(self.config.max_daily_volume_usd)
            .max(0.0)
    }
}

impl Default for RiskManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_trade_limit() {
        let manager = RiskManager::with_config(RiskConfig {
            max_single_trade_usd: 1000.0,
            ..Default::default()
        });

        let context = TradeContext {
            user_id: "user1".to_string(),
            from_token: "USDC".to_string(),
            to_token: "SOL".to_string(),
            amount_usd: 5000.0, // Exceeds limit
            expected_slippage: 0.5,
            liquidity_usd: Some(1_000_000.0),
            is_flagged: false,
        };

        let result = manager.check_trade(&context);
        assert!(result.is_err());
    }

    #[test]
    fn test_approved_trade() {
        let manager = RiskManager::new();

        let context = TradeContext {
            user_id: "user1".to_string(),
            from_token: "USDC".to_string(),
            to_token: "SOL".to_string(),
            amount_usd: 100.0,
            expected_slippage: 0.5,
            liquidity_usd: Some(1_000_000.0),
            is_flagged: false,
        };

        let result = manager.check_trade(&context);
        assert!(result.is_ok());
    }
}
