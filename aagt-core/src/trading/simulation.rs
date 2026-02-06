//! Trade simulation system
//!
//! Allows simulating trades before execution to estimate costs, slippage, etc.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use crate::error::Result;
use async_trait::async_trait;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

/// Result of a trade simulation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationResult {
    /// Whether simulation was successful
    pub success: bool,
    /// Input token
    pub from_token: String,
    /// Output token
    pub to_token: String,
    /// Input amount
    pub input_amount: Decimal,
    /// Expected output amount
    pub output_amount: Decimal,
    /// Estimated price impact percentage
    pub price_impact_percent: Decimal,
    /// Estimated gas cost in USD
    pub gas_cost_usd: Decimal,
    /// Minimum output with slippage
    pub min_output: Decimal,
    /// Exchange/DEX being used
    pub exchange: String,
    /// Route taken (for multi-hop swaps)
    pub route: Vec<String>,
    /// Warnings if any
    pub warnings: Vec<String>,
}

impl SimulationResult {
    /// Check if this trade has high price impact
    pub fn has_high_impact(&self, threshold: Decimal) -> bool {
        self.price_impact_percent > threshold
    }

    /// Get total cost (gas + price impact)
    pub fn total_cost_usd(&self, input_price_usd: Decimal) -> Decimal {
        let impact_cost = self.input_amount * input_price_usd * (self.price_impact_percent / dec!(100.0));
        self.gas_cost_usd + impact_cost
    }
}

/// Request for trade simulation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationRequest {
    /// Token to sell
    pub from_token: String,
    /// Token to buy
    pub to_token: String,
    /// Amount to swap
    pub amount: Decimal,
    /// Slippage tolerance percentage
    pub slippage_tolerance: Decimal,
    /// Chain to simulate on
    pub chain: String,
    /// Optional: specific exchange to use
    pub exchange: Option<String>,
}

/// Trait for implementing simulators
#[async_trait]
pub trait Simulator: Send + Sync {
    /// Simulate a trade
    async fn simulate(&self, request: &SimulationRequest) -> Result<SimulationResult>;

    /// Get supported chains
    fn supported_chains(&self) -> Vec<String>;
}

/// Abstract pricing source for simulations
#[async_trait]
pub trait PriceSource: Send + Sync {
    /// Get exact price in USD
    async fn get_price_usd(&self, token: &str) -> Result<Decimal>;
    /// Get liquidity in USD for a pair
    async fn get_liquidity_usd(&self, token_a: &str, token_b: &str) -> Result<Decimal>;
}

/// Mock Price Source for testing/default
pub struct MockPriceSource;
#[async_trait]
impl PriceSource for MockPriceSource {
    async fn get_price_usd(&self, _token: &str) -> Result<Decimal> { Ok(Decimal::ONE) }
    async fn get_liquidity_usd(&self, _token_a: &str, _token_b: &str) -> Result<Decimal> { Ok(dec!(10_000_000.0)) }
}

/// A basic simulator that estimates based on liquidity
pub struct BasicSimulator {
    /// Default gas cost per chain
    default_gas_usd: Decimal,
    /// Price source
    price_source: Arc<dyn PriceSource>,
}

impl BasicSimulator {
    /// Create with default settings
    pub fn new() -> Self {
        Self {
            default_gas_usd: dec!(0.5),
            price_source: Arc::new(MockPriceSource),
        }
    }

    /// Create with custom price source
    pub fn with_source(source: Arc<dyn PriceSource>) -> Self {
        Self {
            default_gas_usd: dec!(0.5),
            price_source: source,
        }
    }

    /// Estimate price impact based on amount and liquidity
    fn estimate_price_impact(amount_usd: Decimal, liquidity_usd: Decimal) -> Decimal {
        if liquidity_usd.is_zero() {
            return dec!(100.0);
        }
        (amount_usd / liquidity_usd * dec!(100.0)).min(dec!(100.0))
    }
}

impl Default for BasicSimulator {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Simulator for BasicSimulator {
    async fn simulate(&self, request: &SimulationRequest) -> Result<SimulationResult> {
        // 1. Get Prices
        let price_from = self.price_source.get_price_usd(&request.from_token).await.unwrap_or(Decimal::ONE);
        let amount_usd = request.amount * price_from;
        
        let price_to = self.price_source.get_price_usd(&request.to_token).await.unwrap_or(Decimal::ONE);
        
        // 2. Get Liquidity and Impact
        let liquidity = self.price_source.get_liquidity_usd(&request.from_token, &request.to_token)
            .await.unwrap_or(dec!(1000000.0));
            
        let price_impact = Self::estimate_price_impact(amount_usd, liquidity);
        
        // 3. Calculate Output
        let gross_output_tokens = (request.amount * price_from) / price_to;
        let fee_rate = dec!(1.0) - dec!(0.003);
        let impact_rate = dec!(1.0) - (price_impact / dec!(100.0));
        let net_output_tokens = gross_output_tokens * fee_rate * impact_rate;
        
        let min_output = net_output_tokens * (dec!(1.0) - request.slippage_tolerance / dec!(100.0));

        let mut warnings = Vec::new();
        if price_impact > Decimal::ONE {
            warnings.push("High price impact detected".to_string());
        }

        Ok(SimulationResult {
            success: true,
            from_token: request.from_token.clone(),
            to_token: request.to_token.clone(),
            input_amount: request.amount,
            output_amount: net_output_tokens,
            price_impact_percent: price_impact,
            gas_cost_usd: self.default_gas_usd,
            min_output,
            exchange: request.exchange.clone().unwrap_or_else(|| "Jupiter".to_string()),
            route: vec![request.from_token.clone(), request.to_token.clone()],
            warnings,
        })
    }

    fn supported_chains(&self) -> Vec<String> {
        vec!["solana".to_string(), "ethereum".to_string()]
    }
}

/// Multi-chain simulator that delegates to chain-specific simulators
pub struct MultiChainSimulator {
    /// Chain-specific simulators
    simulators: std::collections::HashMap<String, Box<dyn Simulator>>,
}

impl MultiChainSimulator {
    /// Create with no simulators
    pub fn new() -> Self {
        Self {
            simulators: std::collections::HashMap::new(),
        }
    }

    /// Add a chain-specific simulator
    pub fn add_chain(&mut self, chain: impl Into<String>, simulator: Box<dyn Simulator>) {
        self.simulators.insert(chain.into(), simulator);
    }

    /// Simulate on specific chain
    pub async fn simulate_on_chain(
        &self,
        chain: &str,
        request: &SimulationRequest,
    ) -> Result<SimulationResult> {
        let simulator = self
            .simulators
            .get(chain)
            .ok_or_else(|| crate::error::Error::Simulation(format!("Unsupported chain: {}", chain)))?;

        simulator.simulate(request).await
    }
}

impl Default for MultiChainSimulator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_basic_simulation() {
        let simulator = BasicSimulator::new();

        let request = SimulationRequest {
            from_token: "USDC".to_string(),
            to_token: "SOL".to_string(),
            amount: dec!(100.0),
            slippage_tolerance: dec!(1.0),
            chain: "solana".to_string(),
            exchange: None,
        };

        let result = simulator.simulate(&request).await.expect("simulation should succeed");
        
        assert!(result.success);
        assert!(result.output_amount > Decimal::ZERO);
        assert!(result.min_output < result.output_amount);
    }
}
