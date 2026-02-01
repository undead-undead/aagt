//! Trade simulation system
//!
//! Allows simulating trades before execution to estimate costs, slippage, etc.

use serde::{Deserialize, Serialize};

use crate::error::Result;

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
    pub input_amount: f64,
    /// Expected output amount
    pub output_amount: f64,
    /// Estimated price impact percentage
    pub price_impact_percent: f64,
    /// Estimated gas cost in USD
    pub gas_cost_usd: f64,
    /// Minimum output with slippage
    pub min_output: f64,
    /// Exchange/DEX being used
    pub exchange: String,
    /// Route taken (for multi-hop swaps)
    pub route: Vec<String>,
    /// Warnings if any
    pub warnings: Vec<String>,
}

impl SimulationResult {
    /// Check if this trade has high price impact
    pub fn has_high_impact(&self, threshold: f64) -> bool {
        self.price_impact_percent > threshold
    }

    /// Get total cost (gas + price impact)
    pub fn total_cost_usd(&self, input_price_usd: f64) -> f64 {
        let impact_cost = self.input_amount * input_price_usd * (self.price_impact_percent / 100.0);
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
    pub amount: f64,
    /// Slippage tolerance percentage
    pub slippage_tolerance: f64,
    /// Chain to simulate on
    pub chain: String,
    /// Optional: specific exchange to use
    pub exchange: Option<String>,
}

/// Trait for implementing simulators
#[async_trait::async_trait]
pub trait Simulator: Send + Sync {
    /// Simulate a trade
    async fn simulate(&self, request: &SimulationRequest) -> Result<SimulationResult>;

    /// Get supported chains
    fn supported_chains(&self) -> Vec<String>;
}

/// A basic simulator that estimates based on liquidity
pub struct BasicSimulator {
    /// Default gas cost per chain
    default_gas_usd: f64,
}

impl BasicSimulator {
    /// Create with default settings
    pub fn new() -> Self {
        Self {
            default_gas_usd: 0.5,
        }
    }

    /// Estimate price impact based on amount and liquidity
    fn estimate_price_impact(amount: f64, liquidity: f64) -> f64 {
        // Simple constant product formula approximation
        if liquidity <= 0.0 {
            return 100.0; // Max impact for no liquidity
        }
        (amount / liquidity * 100.0).min(100.0)
    }
}

impl Default for BasicSimulator {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Simulator for BasicSimulator {
    async fn simulate(&self, request: &SimulationRequest) -> Result<SimulationResult> {
        // This is a mock implementation
        // In production, this would call actual DEX APIs
        
        let price_impact = 0.3; // Mock 0.3% impact
        let output = request.amount * 0.997; // Mock 0.3% less output
        let min_output = output * (1.0 - request.slippage_tolerance / 100.0);

        let mut warnings = Vec::new();
        if price_impact > 1.0 {
            warnings.push("High price impact detected".to_string());
        }

        Ok(SimulationResult {
            success: true,
            from_token: request.from_token.clone(),
            to_token: request.to_token.clone(),
            input_amount: request.amount,
            output_amount: output,
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
            amount: 100.0,
            slippage_tolerance: 1.0,
            chain: "solana".to_string(),
            exchange: None,
        };

        let result = simulator.simulate(&request).await.expect("simulation should succeed");
        
        assert!(result.success);
        assert!(result.output_amount > 0.0);
        assert!(result.min_output < result.output_amount);
    }
}
