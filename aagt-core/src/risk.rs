//! Risk control system for trading operations
//!
//! Provides safety checks before executing trades.
//! Refactored to use the Actor Model for lock-free concurrency and durability.

use std::sync::Arc;
use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};
use chrono::{DateTime, Utc};
use futures::FutureExt;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::error::{Error, Result};

mod circuit_breaker;
pub use circuit_breaker::DeadManSwitch;

mod checks;
pub use checks::{
    CompositeCheck, LiquidityCheck, MaxTradeAmountCheck, 
    RiskCheckBuilder, SlippageCheck, TokenSecurityCheck,
};

/// Persistence trait for risk state
#[async_trait::async_trait]
pub trait RiskStateStore: Send + Sync {
    async fn load(&self) -> Result<HashMap<String, UserState>>;
    async fn save(&self, states: &HashMap<String, UserState>) -> Result<()>;
}

/// Simple JSON file store for risk state
pub struct FileRiskStore {
    path: PathBuf,
}

impl FileRiskStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

#[async_trait::async_trait]
impl RiskStateStore for FileRiskStore {
    async fn load(&self) -> Result<HashMap<String, UserState>> {
        if !self.path.exists() {
            return Ok(HashMap::new());
        }
        let content = tokio::fs::read_to_string(&self.path).await?;
        if content.trim().is_empty() {
            return Ok(HashMap::new());
        }
        
        serde_json::from_str(&content).map_err(|e| {
            Error::Internal(format!("CORRUPTION: Risk state file at {:?} is malformed. Delete it to reset or fix JSON: {}", self.path, e))
        })
    }

    async fn save(&self, states: &HashMap<String, UserState>) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent).await.ok();
        }

        let path = self.path.clone();
        let states = states.clone(); 

        // Fix #3: Atomic Write Pattern (Write tmp -> Rename)
        tokio::task::spawn_blocking(move || {
            let tmp_path = path.with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));
            
            // Scope for file to ensure it closes before rename
            {
                let file = std::fs::File::create(&tmp_path)
                    .map_err(|e| Error::Internal(format!("Failed to create tmp risk file: {}", e)))?;
                let writer = std::io::BufWriter::new(file);
                
                serde_json::to_writer_pretty(writer, &states)
                    .map_err(|e| Error::Internal(format!("Failed to serialize risk state: {}", e)))?;
                // File closes here
            }

            std::fs::rename(&tmp_path, &path)
                .map_err(|e| {
                    // Try to clean up tmp file if rename fails
                    let _ = std::fs::remove_file(&tmp_path);
                    Error::Internal(format!("Failed to rename risk file: {}", e))
                })?;
            
            Ok::<(), Error>(())
        }).await.map_err(|e| Error::Internal(format!("Join error: {}", e)))??;
        
        Ok(())
    }
}

/// No-op store for in-memory only execution
pub struct InMemoryRiskStore;

#[async_trait::async_trait]
impl RiskStateStore for InMemoryRiskStore {
    async fn load(&self) -> Result<HashMap<String, UserState>> { Ok(HashMap::new()) }
    async fn save(&self, _: &HashMap<String, UserState>) -> Result<()> { Ok(()) }
}

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
    pub max_single_trade_usd: Decimal,
    /// Maximum daily volume usd
    pub max_daily_volume_usd: Decimal,
    /// Maximum slippage percentage allowed
    pub max_slippage_percent: Decimal,
    /// Minimum liquidity required in USD
    pub min_liquidity_usd: Decimal,
    /// Enable rug pull detection
    pub enable_rug_detection: bool,
    /// Cooldown between trades in seconds
    pub trade_cooldown_secs: u64,
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            max_single_trade_usd: dec!(10000.0),
            max_daily_volume_usd: dec!(50000.0),
            max_slippage_percent: dec!(5.0),
            min_liquidity_usd: dec!(100000.0),
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
    pub amount_usd: Decimal,
    /// Expected slippage
    pub expected_slippage: Decimal,
    /// Token liquidity in USD
    pub liquidity_usd: Option<Decimal>,
    /// Is this token flagged as risky
    pub is_flagged: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserState {
    /// Daily volume traded (committed)
    pub daily_volume_usd: Decimal,
    /// Volume currently reserved by pending trades (not yet committed)
    pub pending_volume_usd: Decimal,
    /// Last trade timestamp
    pub last_trade: Option<DateTime<Utc>>,
    /// Volume reset time (Last date processed)
    pub volume_reset: DateTime<Utc>,
}

impl Default for UserState {
    fn default() -> Self {
        Self {
            daily_volume_usd: Decimal::ZERO,
            pending_volume_usd: Decimal::ZERO,
            last_trade: None,
            volume_reset: Utc::now(),
        }
    }
}

// --- Actor Implementation ---

enum RiskCommand {
    CheckAndReserve { context: TradeContext, checks: Vec<Arc<dyn RiskCheck>>, reply: oneshot::Sender<Result<()>> },
    Commit { user_id: String, amount_usd: Decimal, reply: oneshot::Sender<Result<()>> },
    Rollback { user_id: String, amount_usd: Decimal },
    GetRemaining { user_id: String, reply: oneshot::Sender<Decimal> },
    LoadState { reply: oneshot::Sender<Result<()>> },
}

struct RiskActor {
    config: RiskConfig,
    state: HashMap<String, UserState>,
    store: Arc<dyn RiskStateStore>,
    receiver: mpsc::Receiver<RiskCommand>,
    last_load_time: Option<DateTime<Utc>>,
}

impl RiskActor {

    async fn handle_load(&mut self) -> Result<()> {
        let mut loaded = self.store.load().await?;
        
        // Fix #2.1: Clear zombie pending volumes on startup
        // Anything pending during a crash is considered failed/not-executed.
        for (_, state) in loaded.iter_mut() {
            if !state.pending_volume_usd.is_zero() {
                tracing::warn!("Resetting zombie pending volume of ${} for user", state.pending_volume_usd);
                state.pending_volume_usd = Decimal::ZERO;
            }
        }

        self.state = loaded;
        self.last_load_time = Some(Utc::now());
        Ok(())
    }

    async fn handle_check_and_reserve(&mut self, context: TradeContext, checks: Vec<Arc<dyn RiskCheck>>) -> Result<()> {
        // 1. Offload heavy/STATLESS checks to blocking thread
        // These checks don't need UserState (RAM) and could involve I/O in custom checks
        let config = self.config.clone();
        let ctx_clone = context.clone();
        tokio::task::spawn_blocking(move || {
             Self::validate_stateless(&config, &ctx_clone, &checks)
        }).await.map_err(|e| Error::Internal(format!("Task panic: {}", e)))??;

        // 2. Perform STATEFUL checks inside Actor (Atomic)
        let state = self.state.entry(context.user_id.clone()).or_default();
        
        // Reset volume if day changed
        let now = Utc::now();
        if now.date_naive() > state.volume_reset.date_naive() {
            state.daily_volume_usd = Decimal::ZERO;
            state.volume_reset = now;
        }

        // Daily limit check
        let projected = state.daily_volume_usd + state.pending_volume_usd + context.amount_usd;
        if projected > self.config.max_daily_volume_usd {
            return Err(Error::RiskLimitExceeded {
                limit_type: "daily_volume".to_string(),
                current: format!("${:.2}", projected),
                max: format!("${:.2}", self.config.max_daily_volume_usd),
            });
        }

        // Cooldown check
        if let Some(last) = state.last_trade {
            let elapsed = now - last;
            if elapsed < chrono::Duration::seconds(self.config.trade_cooldown_secs as i64) {
                 return Err(Error::risk_check_failed("cooldown", "Trading too fast"));
            }
        }

        // Commit reservation
        state.pending_volume_usd += context.amount_usd;
        
        // Immediate save for reservation
        self.store.save(&self.state).await?;
        
        Ok(())
    }

    /// Stateless validation logic - can be run outside Actor
    fn validate_stateless(config: &RiskConfig, context: &TradeContext, checks: &[Arc<dyn RiskCheck>]) -> Result<()> {
        // Fix #2: Reject negative or zero amounts (Crucial Security Fix)
        if context.amount_usd <= Decimal::ZERO {
             return Err(Error::risk_check_failed("amount_validation", format!("Amount must be positive, got ${:.2}", context.amount_usd)));
        }

        if context.amount_usd > config.max_single_trade_usd {
            return Err(Error::RiskLimitExceeded {
                limit_type: "single_trade".to_string(),
                current: format!("${:.2}", context.amount_usd),
                max: format!("${:.2}", config.max_single_trade_usd),
            });
        }
        if context.expected_slippage > config.max_slippage_percent {
            return Err(Error::risk_check_failed("slippage", format!("Slippage {} > {}", context.expected_slippage, config.max_slippage_percent)));
        }
        if let Some(liq) = context.liquidity_usd {
            if liq < config.min_liquidity_usd {
                return Err(Error::risk_check_failed("liquidity", "Insufficient liquidity"));
            }
        }
        if config.enable_rug_detection && context.is_flagged {
            return Err(Error::risk_check_failed("rug_detection", "Token flagged as risky"));
        }

        for check in checks {
            if let RiskCheckResult::Rejected { reason } = check.check(context) {
                return Err(Error::RiskCheckFailed { check_name: check.name().to_string(), reason });
            }
        }
        Ok(())
    }

    async fn handle_commit(&mut self, user_id: String, amount: Decimal) -> Result<()> {
        let state = self.state.entry(user_id.clone()).or_default();
        
        let old_pending = state.pending_volume_usd;
        let old_daily = state.daily_volume_usd;
        let old_last = state.last_trade;

        state.pending_volume_usd = (state.pending_volume_usd - amount).max(Decimal::ZERO);
        state.daily_volume_usd += amount;
        state.last_trade = Some(Utc::now());

        if let Err(e) = self.store.save(&self.state).await {
            // Rollback on failure
            if let Some(s) = self.state.get_mut(&user_id) {
                s.pending_volume_usd = old_pending;
                s.daily_volume_usd = old_daily;
                s.last_trade = old_last;
            }
            return Err(e);
        }
        Ok(())
    }

    fn handle_rollback(&mut self, user_id: String, amount: Decimal) {
        if let Some(state) = self.state.get_mut(&user_id) {
            state.pending_volume_usd = (state.pending_volume_usd - amount).max(Decimal::ZERO);
        }
    }

    fn handle_get_remaining(&self, user_id: String) -> Decimal {
        if let Some(state) = self.state.get(&user_id) {
             (self.config.max_daily_volume_usd - (state.daily_volume_usd + state.pending_volume_usd)).max(Decimal::ZERO)
        } else {
            self.config.max_daily_volume_usd
        }
    }
}

/// The main risk manager
pub struct RiskManager {
    sender: mpsc::Sender<RiskCommand>,
    /// We keep config copy for easy access if needed, but actor has it too.
    config: RiskConfig,
    /// Custom checks are moved to calls or kept here?
    /// If we keep them here, we have to clone/send them on every check.
    /// `Arc<dyn RiskCheck>` is cheap to clone.
    custom_checks: std::sync::RwLock<Vec<Arc<dyn RiskCheck>>>,
}

impl RiskManager {
    /// Create with default config and in-memory storage (Async)
    pub async fn new() -> Result<Self> {
        Self::with_config(RiskConfig::default(), Arc::new(InMemoryRiskStore)).await
    }

    /// Create with custom config and storage (Async)
    pub async fn with_config(config: RiskConfig, store: Arc<dyn RiskStateStore>) -> Result<Self> {
        let (tx, rx) = mpsc::channel(100);
        
        let actor = RiskActor {
            config: config.clone(),
            state: HashMap::new(),
            store,
            receiver: rx,
            last_load_time: None,
        };
        tokio::spawn(async move {
            let mut actor = actor;
            loop {
                let rx = &mut actor.receiver;
                // If the receiver is closed, the manager is dropped, so we should exit
                if rx.is_closed() {
                    break;
                }

                tracing::info!("RiskActor starting/restarting");
                let res = std::panic::AssertUnwindSafe(async {
                    // We need to re-create the actor if we want to reset some state, 
                    // but here we just want to keep the loop running and handle messages.
                    // The state is already in the `actor` struct.
                    
                    // Actually, if it panics, we might want to reload state from disk
                    // but we have to be careful about pending volume.
                    // For now, just keep the task alive.
                    // Fix: Track if state was modified during message processing
                    let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
                    let mut dirty = false;  // Fix L2: Track if state needs saving
                    
                    loop {
                        tokio::select! {
                            maybe_msg = actor.receiver.recv() => {
                                match maybe_msg {
                                    Some(msg) => {
                                         match msg {
                                             RiskCommand::CheckAndReserve { context, checks, reply } => {
                                                 // Moved checks into the handler
                                                 let res = actor.handle_check_and_reserve(context, checks).await;
                                                 dirty = res.is_ok();  // Mark dirty if reservation succeeded
                                                 let _ = reply.send(res);
                                             }
                                             RiskCommand::Commit { user_id, amount_usd, reply } => {
                                                 let res = actor.handle_commit(user_id, amount_usd).await;
                                                 // Commit already saves, no need to set dirty
                                                 let _ = reply.send(res);
                                             }
                                             RiskCommand::Rollback { user_id, amount_usd } => {
                                                 actor.handle_rollback(user_id, amount_usd);
                                                 dirty = true;
                                             }
                                             RiskCommand::GetRemaining { user_id, reply } => {
                                                 let val = actor.handle_get_remaining(user_id);
                                                 let _ = reply.send(val);
                                             }
                                             RiskCommand::LoadState { reply } => {
                                                 let res = actor.handle_load().await;
                                                 let _ = reply.send(res);
                                             }
                                         }
                                    }
                                    None => break, // Channel closed
                                }
                            }
                            _ = interval.tick() => {
                                // Fix L2: Only save if state was modified
                                if dirty {
                                    tracing::debug!("RiskManager: performing periodic state flush");
                                    if let Err(e) = actor.store.save(&actor.state).await {
                                         tracing::error!("Periodic risk persistence failed: {}", e);
                                    } else {
                                        dirty = false;
                                    }
                                }
                            }
                        }
                    }
                }).catch_unwind().await;

                if let Err(_) = res {
                    tracing::error!("RiskActor PANICKED. Restarting in 1s...");
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                } else {
                    // Normal exit (sender dropped)
                    break;
                }
            }
        });

        let manager = Self {
            sender: tx,
            config,
            custom_checks: std::sync::RwLock::new(Vec::new()),
        };
        
        // Fix #1: Auto-load state on startup
        manager.load_state().await?;
        
        Ok(manager)
    }
    
    /// Backward compatible Strict constructor (already strict, now matches new behavior but keeps name)
    pub async fn new_strict(config: RiskConfig, store: Arc<dyn RiskStateStore>) -> Result<Self> {
        Self::with_config(config, store).await
    }
    
    // ... load_state and other methods remain ...

    /// Load state from store
    pub async fn load_state(&self) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.sender.send(RiskCommand::LoadState { reply: tx }).await
            .map_err(|_| Error::Internal("Risk actor closed".to_string()))?;
        rx.await.map_err(|_| Error::Internal("Risk actor dropped reply".to_string()))?
    }

    /// Add a custom risk check
    pub fn add_check(&self, check: Arc<dyn RiskCheck>) {
        if let Ok(mut checks) = self.custom_checks.write() {
            checks.push(check);
        }
    }

    /// Perform all risk checks for a trade AND reserve the volume.
    pub async fn check_and_reserve(&self, context: &TradeContext) -> Result<()> {
        let checks = self.custom_checks.read()
            .map_err(|_| Error::Internal("Risk check lock poisoned".to_string()))?
            .clone();
        
        let (tx, rx) = oneshot::channel();
        self.sender.send(RiskCommand::CheckAndReserve { 
            context: context.clone(), 
            checks, 
            reply: tx 
        }).await.map_err(|_| Error::Internal("Risk actor closed".to_string()))?;
        
        rx.await.map_err(|_| Error::Internal("Risk actor dropped reply".to_string()))?
    }

    /// Backward compatible check
    #[deprecated(note = "Use check_and_reserve for race-condition safety")]
    pub async fn check_trade(&self, context: &TradeContext) -> Result<()> {
        self.check_and_reserve(context).await?;
        self.rollback_trade(&context.user_id, context.amount_usd).await;
        Ok(())
    }

    /// Commit a trade that was previously reserved
    pub async fn commit_trade(&self, user_id: &str, amount_usd: Decimal) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.sender.send(RiskCommand::Commit { 
            user_id: user_id.to_string(), 
            amount_usd, 
            reply: tx 
        }).await.map_err(|_| Error::Internal("Risk actor closed".to_string()))?;
        
        rx.await.map_err(|_| Error::Internal("Risk actor dropped reply".to_string()))?
    }

    /// Rollback a reservation
    pub async fn rollback_trade(&self, user_id: &str, amount_usd: Decimal) {
        let _ = self.sender.send(RiskCommand::Rollback { 
            user_id: user_id.to_string(), 
            amount_usd 
        }).await;
    }

    /// Record a trade immediately
    pub async fn record_trade(&self, user_id: &str, amount_usd: Decimal) -> Result<()> {
        self.commit_trade(user_id, amount_usd).await
    }
    
    /// Get remaining daily limit for a user
    pub async fn remaining_daily_limit(&self, user_id: &str) -> Decimal {
        let (tx, rx) = oneshot::channel();
        if let Err(_) = self.sender.send(RiskCommand::GetRemaining { 
            user_id: user_id.to_string(), 
            reply: tx 
        }).await {
            return Decimal::ZERO;
        }
        rx.await.unwrap_or(Decimal::ZERO)
    }
}

// Default trait removed because new() is async. Use RiskManager::new().await instead.

// Tests kept but might need async adjustment if logic changed (it mostly didn't, just interface)
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_single_trade_limit() {
        let manager = RiskManager::with_config(
            RiskConfig {
                max_single_trade_usd: dec!(1000.0),
                ..Default::default()
            },
            Arc::new(InMemoryRiskStore),
        ).await.unwrap();

        let context = TradeContext {
            user_id: "user1".to_string(),
            from_token: "USDC".to_string(),
            to_token: "SOL".to_string(),
            amount_usd: dec!(5000.0),
            expected_slippage: dec!(0.5),
            liquidity_usd: Some(dec!(1_000_000.0)),
            is_flagged: false,
        };

        let result = manager.check_and_reserve(&context).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_reserve_commit_flow() {
        let manager = RiskManager::new().await.unwrap();

        let context = TradeContext {
            user_id: "user1".to_string(),
            from_token: "USDC".to_string(),
            to_token: "SOL".to_string(),
            amount_usd: dec!(100.0),
            expected_slippage: dec!(0.5),
            liquidity_usd: Some(dec!(1_000_000.0)),
            is_flagged: false,
        };

        // 1. Reserve
        assert!(manager.check_and_reserve(&context).await.is_ok());
        
        // 2. Commit
        manager.commit_trade("user1", dec!(100.0)).await.unwrap();
        
        // 3. Check remaining
        let remaining = manager.remaining_daily_limit("user1").await;
        assert_eq!(remaining, dec!(50_000.0) - dec!(100.0));
    }
}
