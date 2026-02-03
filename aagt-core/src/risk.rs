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

use crate::error::{Error, Result};

mod circuit_breaker;
pub use circuit_breaker::DeadManSwitch;

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

        tokio::task::spawn_blocking(move || {
            let tmp_path = path.with_extension("tmp");
            let file = std::fs::File::create(&tmp_path)
                .map_err(|e| Error::Internal(format!("Failed to create tmp risk file: {}", e)))?;
            let writer = std::io::BufWriter::new(file);

            serde_json::to_writer_pretty(writer, &states)
                .map_err(|e| Error::Internal(format!("Failed to serialize risk state: {}", e)))?;

            std::fs::rename(tmp_path, &path)
                .map_err(|e| Error::Internal(format!("Failed to rename risk file: {}", e)))?;
            
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserState {
    /// Daily volume traded (committed)
    pub daily_volume_usd: f64,
    /// Volume currently reserved by pending trades (not yet committed)
    #[serde(skip)]
    pub pending_volume_usd: f64,
    /// Last trade timestamp
    pub last_trade: Option<DateTime<Utc>>,
    /// Volume reset time
    pub volume_reset: DateTime<Utc>,
}

impl Default for UserState {
    fn default() -> Self {
        Self {
            daily_volume_usd: 0.0,
            pending_volume_usd: 0.0,
            last_trade: None,
            volume_reset: Utc::now(),
        }
    }
}

// --- Actor Implementation ---

enum RiskCommand {
    CheckAndReserve { context: TradeContext, checks: Vec<Arc<dyn RiskCheck>>, reply: oneshot::Sender<Result<()>> },
    Commit { user_id: String, amount_usd: f64, reply: oneshot::Sender<Result<()>> },
    Rollback { user_id: String, amount_usd: f64 },
    GetRemaining { user_id: String, reply: oneshot::Sender<f64> },
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
        let loaded = self.store.load().await?;
        self.state = loaded;
        self.last_load_time = Some(Utc::now());
        Ok(())
    }

    async fn handle_check_and_reserve(&mut self, context: TradeContext, checks: &[Arc<dyn RiskCheck>]) -> Result<()> {
         // 0. Multi-process reload check (Simple version: always reload or check timestamp)
         // For now, let's just reload to be 100% safe in multi-process scenarios
         // 0. Multi-process reload check REMOVED for performance and InMemory correctness.
         // If you need multi-process sync, use a database or implement a file watcher.
         // let _ = self.handle_load().await;

         // 1. Stateless Checks
        if context.amount_usd > self.config.max_single_trade_usd {
            return Err(Error::RiskLimitExceeded {
                limit_type: "single_trade".to_string(),
                current: format!("${:.2}", context.amount_usd),
                max: format!("${:.2}", self.config.max_single_trade_usd),
            });
        }
        if context.expected_slippage > self.config.max_slippage_percent {
            return Err(Error::risk_check_failed("slippage", format!("Slippage {:.2}% > {:.2}%", context.expected_slippage, self.config.max_slippage_percent)));
        }
        if let Some(liq) = context.liquidity_usd {
            if liq < self.config.min_liquidity_usd {
                return Err(Error::risk_check_failed("liquidity", "Low liquidity"));
            }
        }
        if self.config.enable_rug_detection && context.is_flagged {
             return Err(Error::risk_check_failed("rug_detection", "Token flagged"));
        }

        // 2. Custom Checks
        for check in checks {
            if let RiskCheckResult::Rejected { reason } = check.check(&context) {
                return Err(Error::RiskCheckFailed { check_name: check.name().to_string(), reason });
            }
        }

        // 3. Stateful Checks
        let state = self.state.entry(context.user_id.clone()).or_default();

        if Utc::now() - state.volume_reset > chrono::Duration::seconds(86400) {
            state.daily_volume_usd = 0.0;
            state.pending_volume_usd = 0.0;
            state.volume_reset = Utc::now();
        }

        let projected = state.daily_volume_usd + state.pending_volume_usd + context.amount_usd;
        if projected > self.config.max_daily_volume_usd {
             return Err(Error::RiskLimitExceeded {
                limit_type: "daily_volume".to_string(),
                current: format!("${:.2}", projected),
                max: format!("${:.2}", self.config.max_daily_volume_usd),
            });
        }

        if let Some(last) = state.last_trade {
             let cd = self.config.trade_cooldown_secs as i64;
             let elapsed = Utc::now() - last;
             if elapsed < chrono::Duration::seconds(cd) {
                 return Err(Error::risk_check_failed("cooldown", "Cooldown active"));
             }
        }

        // Reserve
        state.pending_volume_usd += context.amount_usd;
        Ok(())
    }

    async fn handle_commit(&mut self, user_id: String, amount: f64) -> Result<()> {
        let state = self.state.entry(user_id).or_default();
        state.pending_volume_usd = (state.pending_volume_usd - amount).max(0.0);
        state.daily_volume_usd += amount;
        state.last_trade = Some(Utc::now());

        // FORCE SAVE (Durability)
        if let Err(e) = self.store.save(&self.state).await {
            tracing::error!("CRITICAL: Failed to persist risk state: {}", e);
            // We still succeed the Commit in memory, but warn heavily
        }
        Ok(())
    }

    fn handle_rollback(&mut self, user_id: String, amount: f64) {
        if let Some(state) = self.state.get_mut(&user_id) {
            state.pending_volume_usd = (state.pending_volume_usd - amount).max(0.0);
        }
    }

    fn handle_get_remaining(&self, user_id: String) -> f64 {
        if let Some(state) = self.state.get(&user_id) {
             (self.config.max_daily_volume_usd - (state.daily_volume_usd + state.pending_volume_usd)).max(0.0)
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
    /// Create with default config and in-memory storage
    pub fn new() -> Self {
        Self::with_config(RiskConfig::default(), Arc::new(InMemoryRiskStore))
    }

    /// Create with custom config and storage
    pub fn with_config(config: RiskConfig, store: Arc<dyn RiskStateStore>) -> Self {
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
                    while let Some(msg) = actor.receiver.recv().await {
                         match msg {
                             RiskCommand::CheckAndReserve { context, checks, reply } => {
                                 let res = actor.handle_check_and_reserve(context, &checks).await;
                                 let _ = reply.send(res);
                             }
                             RiskCommand::Commit { user_id, amount_usd, reply } => {
                                 let res = actor.handle_commit(user_id, amount_usd).await;
                                 let _ = reply.send(res);
                             }
                             RiskCommand::Rollback { user_id, amount_usd } => {
                                 actor.handle_rollback(user_id, amount_usd);
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

        Self {
            sender: tx,
            config,
            custom_checks: std::sync::RwLock::new(Vec::new()),
        }
    }
    
    /// Create and wait for state to be loaded
    pub async fn new_strict(config: RiskConfig, store: Arc<dyn RiskStateStore>) -> Result<Self> {
        let manager = Self::with_config(config, store);
        manager.load_state().await?;
        Ok(manager)
    }

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
        let checks = self.custom_checks.read().unwrap().clone();
        
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
    pub async fn commit_trade(&self, user_id: &str, amount_usd: f64) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.sender.send(RiskCommand::Commit { 
            user_id: user_id.to_string(), 
            amount_usd, 
            reply: tx 
        }).await.map_err(|_| Error::Internal("Risk actor closed".to_string()))?;
        
        rx.await.map_err(|_| Error::Internal("Risk actor dropped reply".to_string()))?
    }

    /// Rollback a reservation
    pub async fn rollback_trade(&self, user_id: &str, amount_usd: f64) {
        let _ = self.sender.send(RiskCommand::Rollback { 
            user_id: user_id.to_string(), 
            amount_usd 
        }).await;
    }

    /// Record a trade immediately
    pub async fn record_trade(&self, user_id: &str, amount_usd: f64) -> Result<()> {
        self.commit_trade(user_id, amount_usd).await
    }
    
    /// Get remaining daily limit for a user
    pub async fn remaining_daily_limit(&self, user_id: &str) -> f64 {
        let (tx, rx) = oneshot::channel();
        if let Err(_) = self.sender.send(RiskCommand::GetRemaining { 
            user_id: user_id.to_string(), 
            reply: tx 
        }).await {
            return 0.0;
        }
        rx.await.unwrap_or(0.0)
    }
}

impl Default for RiskManager {
    fn default() -> Self {
        Self::new()
    }
}

// Tests kept but might need async adjustment if logic changed (it mostly didn't, just interface)
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_single_trade_limit() {
        let manager = RiskManager::with_config(
            RiskConfig {
                max_single_trade_usd: 1000.0,
                ..Default::default()
            },
            Arc::new(InMemoryRiskStore),
        );

        let context = TradeContext {
            user_id: "user1".to_string(),
            from_token: "USDC".to_string(),
            to_token: "SOL".to_string(),
            amount_usd: 5000.0,
            expected_slippage: 0.5,
            liquidity_usd: Some(1_000_000.0),
            is_flagged: false,
        };

        #[allow(deprecated)]
        let result = manager.check_trade(&context).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_reserve_commit_flow() {
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

        // 1. Reserve
        assert!(manager.check_and_reserve(&context).await.is_ok());
        
        // 2. Commit
        manager.commit_trade("user1", 100.0).await.unwrap();
        
        // 3. Check remaining
        let remaining = manager.remaining_daily_limit("user1").await;
        assert_eq!(remaining, 50_000.0 - 100.0);
    }
}
