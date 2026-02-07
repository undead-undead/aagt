//! Enhanced Risk Check system with composable checks

use super::{RiskCheck, RiskCheckResult, TradeContext};
use rust_decimal::Decimal;
use std::sync::Arc;

/// Maximum trade amount check
pub struct MaxTradeAmountCheck {
    max_amount: Decimal,
}

impl MaxTradeAmountCheck {
    pub fn new(max_amount: Decimal) -> Self {
        Self { max_amount }
    }
}

impl RiskCheck for MaxTradeAmountCheck {
    fn name(&self) -> &str {
        "max_trade_amount"
    }

    fn check(&self, context: &TradeContext) -> RiskCheckResult {
        if context.amount_usd > self.max_amount {
            RiskCheckResult::Rejected {
                reason: format!(
                    "Trade amount ${} exceeds maximum ${}",
                    context.amount_usd, self.max_amount
                ),
            }
        } else {
            RiskCheckResult::Approved
        }
    }
}

/// Slippage tolerance check
pub struct SlippageCheck {
    max_slippage_percent: Decimal,
}

impl SlippageCheck {
    pub fn new(max_slippage_percent: Decimal) -> Self {
        Self {
            max_slippage_percent,
        }
    }
}

impl RiskCheck for SlippageCheck {
    fn name(&self) -> &str {
        "slippage"
    }

    fn check(&self, context: &TradeContext) -> RiskCheckResult {
        if context.expected_slippage > self.max_slippage_percent {
            RiskCheckResult::Rejected {
                reason: format!(
                    "Slippage {}% exceeds maximum {}%",
                    context.expected_slippage, self.max_slippage_percent
                ),
            }
        } else {
            RiskCheckResult::Approved
        }
    }
}

/// Liquidity check
pub struct LiquidityCheck {
    min_liquidity: Decimal,
}

impl LiquidityCheck {
    pub fn new(min_liquidity: Decimal) -> Self {
        Self { min_liquidity }
    }
}

impl RiskCheck for LiquidityCheck {
    fn name(&self) -> &str {
        "liquidity"
    }

    fn check(&self, context: &TradeContext) -> RiskCheckResult {
        match context.liquidity_usd {
            Some(liq) if liq < self.min_liquidity => RiskCheckResult::Rejected {
                reason: format!("Liquidity ${} below minimum ${}", liq, self.min_liquidity),
            },
            None => RiskCheckResult::PendingReview {
                reason: "Liquidity data unavailable".to_string(),
            },
            _ => RiskCheckResult::Approved,
        }
    }
}

/// Token security check
pub struct TokenSecurityCheck {
    blacklist: Vec<String>,
}

impl TokenSecurityCheck {
    pub fn new(blacklist: Vec<String>) -> Self {
        Self { blacklist }
    }
}

impl RiskCheck for TokenSecurityCheck {
    fn name(&self) -> &str {
        "token_security"
    }

    fn check(&self, context: &TradeContext) -> RiskCheckResult {
        if context.is_flagged {
            return RiskCheckResult::Rejected {
                reason: "Token is flagged as risky".to_string(),
            };
        }

        if self.blacklist.contains(&context.to_token) {
            return RiskCheckResult::Rejected {
                reason: format!("Token {} is blacklisted", context.to_token),
            };
        }

        RiskCheckResult::Approved
    }
}

/// Composite check that combines multiple checks
pub struct CompositeCheck {
    checks: Vec<Arc<dyn RiskCheck>>,
    name: String,
}

impl CompositeCheck {
    pub fn new(name: String, checks: Vec<Arc<dyn RiskCheck>>) -> Self {
        Self { name, checks }
    }
}

impl RiskCheck for CompositeCheck {
    fn name(&self) -> &str {
        &self.name
    }

    fn check(&self, context: &TradeContext) -> RiskCheckResult {
        for check in &self.checks {
            match check.check(context) {
                RiskCheckResult::Approved => continue,
                other => return other,
            }
        }
        RiskCheckResult::Approved
    }
}

/// Builder for creating risk check pipelines
pub struct RiskCheckBuilder {
    checks: Vec<Arc<dyn RiskCheck>>,
}

impl RiskCheckBuilder {
    pub fn new() -> Self {
        Self { checks: Vec::new() }
    }

    pub fn add_check(mut self, check: Arc<dyn RiskCheck>) -> Self {
        self.checks.push(check);
        self
    }

    pub fn max_trade_amount(self, max: Decimal) -> Self {
        self.add_check(Arc::new(MaxTradeAmountCheck::new(max)))
    }

    pub fn max_slippage(self, max_percent: Decimal) -> Self {
        self.add_check(Arc::new(SlippageCheck::new(max_percent)))
    }

    pub fn min_liquidity(self, min: Decimal) -> Self {
        self.add_check(Arc::new(LiquidityCheck::new(min)))
    }

    pub fn token_security(self, blacklist: Vec<String>) -> Self {
        self.add_check(Arc::new(TokenSecurityCheck::new(blacklist)))
    }

    pub fn build(self) -> Vec<Arc<dyn RiskCheck>> {
        self.checks
    }

    pub fn build_composite(self, name: String) -> Arc<dyn RiskCheck> {
        Arc::new(CompositeCheck::new(name, self.checks))
    }
}

impl Default for RiskCheckBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_risk_check_builder() {
        let checks = RiskCheckBuilder::new()
            .max_trade_amount(dec!(1000.0))
            .max_slippage(dec!(2.0))
            .min_liquidity(dec!(100000.0))
            .build();

        assert_eq!(checks.len(), 3);

        let context = TradeContext {
            user_id: "test".to_string(),
            from_token: "USDC".to_string(),
            to_token: "SOL".to_string(),
            amount_usd: dec!(500.0),
            expected_slippage: dec!(1.0),
            liquidity_usd: Some(dec!(200000.0)),
            is_flagged: false,
        };

        for check in &checks {
            assert!(check.check(&context).is_approved());
        }
    }

    #[test]
    fn test_composite_check() {
        let composite = RiskCheckBuilder::new()
            .max_trade_amount(dec!(1000.0))
            .max_slippage(dec!(2.0))
            .build_composite("test_composite".to_string());

        let good_context = TradeContext {
            user_id: "test".to_string(),
            from_token: "USDC".to_string(),
            to_token: "SOL".to_string(),
            amount_usd: dec!(500.0),
            expected_slippage: dec!(1.0),
            liquidity_usd: Some(dec!(200000.0)),
            is_flagged: false,
        };

        assert!(composite.check(&good_context).is_approved());

        let bad_context = TradeContext {
            amount_usd: dec!(2000.0),
            ..good_context
        };

        assert!(!composite.check(&bad_context).is_approved());
    }
}
