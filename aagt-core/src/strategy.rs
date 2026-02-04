//! Strategy and pipeline system for automated trading

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::error::Result;
use crate::pipeline::{self, Step, Context};

/// A condition that can trigger a strategy
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Condition {
    /// Price crosses above threshold
    PriceAbove {
        token: String,
        threshold: f64,
    },
    /// Price crosses below threshold
    PriceBelow {
        token: String,
        threshold: f64,
    },
    /// Price changes by percentage
    PriceChange {
        token: String,
        percent: f64,
        direction: PriceDirection,
    },
    /// Time-based trigger
    Schedule {
        cron: String,
    },
    /// Manual trigger
    Manual,
    /// All conditions must be true
    And(Vec<Condition>),
    /// Any condition must be true
    Or(Vec<Condition>),
}

/// Direction of price change
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PriceDirection {
    Up,
    Down,
    Any,
}

/// An action to execute
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    /// Swap tokens
    Swap {
        from_token: String,
        to_token: String,
        amount: String, // Can be "100" or "50%" or "max"
    },
    /// Send notification
    Notify {
        channel: NotifyChannel,
        message: String,
    },
    /// Wait for duration
    Wait {
        seconds: u64,
    },
    /// Cancel pipeline
    Cancel {
        reason: String,
    },
}

/// Notification channel
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotifyChannel {
    Email,
    Telegram,
    Discord,
    Webhook { url: String },
}

/// A trading strategy/pipeline
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Strategy {
    /// Unique ID
    pub id: String,
    /// User who owns this strategy
    pub user_id: String,
    /// Name of the strategy
    pub name: String,
    /// Description
    pub description: Option<String>,
    /// Trigger condition
    pub condition: Condition,
    /// Actions to execute
    pub actions: Vec<Action>,
    /// Is strategy active
    pub active: bool,
    /// Created timestamp
    pub created_at: i64,
}

/// Status of a pipeline execution
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineStatus {
    /// Waiting for trigger
    Pending,
    /// Currently running
    Running,
    /// Completed successfully
    Completed,
    /// Failed with error
    Failed { error: String },
    /// Cancelled
    Cancelled { reason: String },
}

/// A pipeline execution record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pipeline {
    /// Execution ID
    pub id: String,
    /// Strategy ID
    pub strategy_id: String,
    /// User ID
    pub user_id: String,
    /// Current status
    pub status: PipelineStatus,
    /// Current step index
    pub current_step: usize,
    /// Results from each step
    pub step_results: Vec<StepResult>,
    /// Started at
    pub started_at: i64,
    /// Completed at
    pub completed_at: Option<i64>,
}

/// Result of a pipeline step
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    /// Step index
    pub index: usize,
    /// Action that was executed
    pub action: Action,
    /// Success or failure
    pub success: bool,
    /// Result message
    pub message: String,
    /// Timestamp
    pub timestamp: i64,
}

/// Trait for condition evaluators
#[async_trait::async_trait]
pub trait ConditionEvaluator: Send + Sync {
    /// Evaluate if condition is met
    async fn evaluate(&self, condition: &Condition) -> Result<bool>;
}

#[async_trait::async_trait]
pub trait ActionExecutor: Send + Sync {
    /// Execute an action
    async fn execute(&self, action: &Action, context: &pipeline::Context) -> Result<String>;
}

/// Adapter to run a strategy Action as a pipeline Step
pub struct ActionStep {
    action: Action,
    executor: Arc<dyn ActionExecutor>,
}

impl ActionStep {
    pub fn new(action: Action, executor: Arc<dyn ActionExecutor>) -> Self {
        Self { action, executor }
    }
}

#[async_trait::async_trait]
impl Step for ActionStep {
    async fn execute(&self, ctx: &mut Context) -> anyhow::Result<()> {
        let res = self.executor.execute(&self.action, ctx).await?;
        ctx.log(format!("Action '{}' result: {}", self.name(), res));
        Ok(())
    }

    fn name(&self) -> &str {
        match &self.action {
            Action::Swap { .. } => "swap",
            Action::Notify { .. } => "notify",
            Action::Wait { .. } => "wait",
            Action::Cancel { .. } => "cancel",
        }
    }
}



/// Persistence for strategies
#[async_trait::async_trait]
pub trait StrategyStore: Send + Sync {
    /// Load all active strategies
    async fn load(&self) -> Result<Vec<Strategy>>;
    /// Save a strategy (create or update)
    async fn save(&self, strategy: &Strategy) -> Result<()>;
    /// Delete a strategy
    async fn delete(&self, id: &str) -> Result<()>;
}

/// Simple JSON file store for strategies
pub struct FileStrategyStore {
    path: std::path::PathBuf,
    /// Lock to prevent race conditions during read-modify-write cycles
    lock: tokio::sync::Mutex<()>,
}

impl FileStrategyStore {
    pub fn new(path: impl Into<std::path::PathBuf>) -> Self {
        Self { 
            path: path.into(),
            lock: tokio::sync::Mutex::new(()),
        }
    }
    
    // Helper to open file with exclusive lock
    async fn with_lock<F, T>(&self, f: F) -> Result<T> 
    where F: FnOnce() -> Result<T> + Send + 'static, T: Send + 'static 
    {
        let path = self.path.clone();
        
        // Blocking I/O for file locking
        tokio::task::spawn_blocking(move || {
            // Ensure parent dir exists
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| crate::error::Error::Internal(e.to_string()))?;
            }
            
            // Open (or create) file for locking
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .open(&path)
                .map_err(|e| crate::error::Error::Internal(format!("Failed to open strategy file: {}", e)))?;
                
            // Exclusive Lock
            use fs2::FileExt;
            file.lock_exclusive().map_err(|e| crate::error::Error::Internal(format!("Failed to lock strategy file: {}", e)))?;
            
            let res = f();
            
            // Unlock happens automatically when file is closed (dropped)
            file.unlock().ok(); 
            
            res
        }).await.map_err(|e| crate::error::Error::Internal(format!("Task join error: {}", e)))?
    }
}

#[async_trait::async_trait]
impl StrategyStore for FileStrategyStore {
    async fn load(&self) -> Result<Vec<Strategy>> {
        // Load doesn't necessarily need exclusive lock if we are okay with potentially stale data,
        // but for consistency let's use shared lock or just read. 
        // For simplicity and avoiding blocking readers, we just read. 
        // If we want strict consistency, we should use lock_shared.
        
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let content = tokio::fs::read_to_string(&self.path).await?;
        if content.trim().is_empty() {
            return Ok(Vec::new());
        }
        let strategies: Vec<Strategy> = serde_json::from_str(&content)
            .map_err(|e| crate::error::Error::Internal(format!("Failed to parse strategy file: {}", e)))?;
        Ok(strategies)
    }

    async fn save(&self, strategy: &Strategy) -> Result<()> {
        let path = self.path.clone();
        let strategy = strategy.clone();
        
        tokio::task::spawn_blocking(move || {
             if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| crate::error::Error::Internal(e.to_string()))?;
            }
            
            let mut file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .open(&path)
                .map_err(|e| crate::error::Error::Internal(format!("Failed to open: {}", e)))?;
                
            use fs2::FileExt;
            file.lock_exclusive().map_err(|e| crate::error::Error::Internal(format!("Failed to lock: {}", e)))?;
            
            // Read current content from file handle
            use std::io::{Read, Seek, SeekFrom, Write};
            let mut content = String::new();
            file.read_to_string(&mut content).map_err(|e| crate::error::Error::Internal(format!("Read failed: {}", e)))?;
            
            let mut strategies: Vec<Strategy> = if content.trim().is_empty() {
                Vec::new()
            } else {
                serde_json::from_str(&content).unwrap_or_else(|_| Vec::new())
            };
            
            // Update
            if let Some(pos) = strategies.iter().position(|s| s.id == strategy.id) {
                strategies[pos] = strategy;
            } else {
                strategies.push(strategy);
            }
            
            // Write back (Truncate/Seek Start)
            file.seek(SeekFrom::Start(0)).map_err(|e| crate::error::Error::Internal(format!("Seek failed: {}", e)))?;
            file.set_len(0).map_err(|e| crate::error::Error::Internal(format!("Truncate failed: {}", e)))?;
            
            serde_json::to_writer_pretty(&file, &strategies)
                .map_err(|e| crate::error::Error::Internal(format!("Serialization error: {}", e)))?;
                
            file.unlock().ok();
            Ok(())
        }).await.map_err(|e| crate::error::Error::Internal(format!("Join error: {}", e)))?
    }

    async fn delete(&self, id: &str) -> Result<()> {
        let path = self.path.clone();
        let id = id.to_string();
        
        tokio::task::spawn_blocking(move || {
            if !path.exists() { return Ok(()); }
            
            let mut file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path)
                .map_err(|e| crate::error::Error::Internal(format!("Failed to open: {}", e)))?;
                
            use fs2::FileExt;
            file.lock_exclusive().map_err(|e| crate::error::Error::Internal(format!("Failed to lock: {}", e)))?;
            
            let mut content = String::new();
            use std::io::{Read, Seek, SeekFrom};
            file.read_to_string(&mut content).map_err(|e| crate::error::Error::Internal(format!("Read failed: {}", e)))?;
            
            if !content.trim().is_empty() {
                let mut strategies: Vec<Strategy> = serde_json::from_str(&content).unwrap_or_default();
                if let Some(pos) = strategies.iter().position(|s| s.id == id) {
                    strategies.remove(pos);
                    
                    file.seek(SeekFrom::Start(0)).map_err(|e| crate::error::Error::Internal(format!("Seek failed: {}", e)))?;
                    file.set_len(0).map_err(|e| crate::error::Error::Internal(format!("Truncate failed: {}", e)))?;
                    
                    serde_json::to_writer_pretty(&file, &strategies)
                       .map_err(|e| crate::error::Error::Internal(format!("Serialization error: {}", e)))?;
                }
            }
            
            file.unlock().ok();
            Ok(())
        }).await.map_err(|e| crate::error::Error::Internal(format!("Join error: {}", e)))?
    }
}


/// In-memory no-op store
pub struct InMemoryStrategyStore;

#[async_trait::async_trait]
impl StrategyStore for InMemoryStrategyStore {
    async fn load(&self) -> Result<Vec<Strategy>> { Ok(Vec::new()) }
    async fn save(&self, _strategy: &Strategy) -> Result<()> { Ok(()) }
    async fn delete(&self, _id: &str) -> Result<()> { Ok(()) }
}

/// Strategy engine for managing and executing strategies
pub struct StrategyEngine {
    /// Condition evaluator
    evaluator: Arc<dyn ConditionEvaluator>,
    /// Action executor
    executor: Arc<dyn ActionExecutor>,
    /// Strategy persistence
    store: Arc<dyn StrategyStore>,
    /// Shutdown signal receiver
    shutdown_rx: Option<mpsc::Receiver<()>>,
}

impl StrategyEngine {
    /// Create a new strategy engine
    pub fn new(
        evaluator: Arc<dyn ConditionEvaluator>,
        executor: Arc<dyn ActionExecutor>,
        store: Arc<dyn StrategyStore>,
    ) -> Self {
        Self {
            evaluator,
            executor,
            store,
            shutdown_rx: None,
        }
    }
    
    /// Create with default in-memory store (backward compatibility helpers)
    pub fn simple(
        evaluator: Arc<dyn ConditionEvaluator>,
        executor: Arc<dyn ActionExecutor>,
    ) -> Self {
        Self::new(evaluator, executor, Arc::new(InMemoryStrategyStore))
    }

    /// Set shutdown signal channel
    pub fn with_shutdown(mut self, rx: mpsc::Receiver<()>) -> Self {
        self.shutdown_rx = Some(rx);
        self
    }
    
    /// Load all active strategies from store
    pub async fn load_active_strategies(&self) -> Result<Vec<Strategy>> {
        let strategies = self.store.load().await?;
        Ok(strategies.into_iter().filter(|s| s.active).collect())
    }
    
    /// Save/Register a strategy
    pub async fn register_strategy(&self, strategy: Strategy) -> Result<()> {
        self.store.save(&strategy).await
    }
    
    /// Delete a strategy
    pub async fn remove_strategy(&self, id: &str) -> Result<()> {
        self.store.delete(id).await
    }

    /// Execute a pipeline with timeout and graceful shutdown
    pub async fn execute_pipeline(
        &self,
        strategy: &Strategy,
        pipeline_id: String,
    ) -> Result<Pipeline> {
        let now = chrono::Utc::now().timestamp();
        
        // 1. Build the generic pipeline
        let mut generic_pipeline = pipeline::Pipeline::new(&strategy.name);
        
        for action in &strategy.actions {
            let step = ActionStep::new(action.clone(), self.executor.clone());
            generic_pipeline = generic_pipeline.add_step(step);
        }

        // 2. Prepare Context
        let mut ctx = Context::new(format!("Strategy execution: {}", strategy.name));
        ctx.set("user_id", strategy.user_id.clone());
        ctx.set("strategy_id", strategy.id.clone());
        ctx.set("pipeline_id", pipeline_id.clone());

        // 3. Run (using shared logic from pipeline.rs)
        let result_ctx = generic_pipeline.run_with_context(ctx).await
            .map_err(|e| crate::error::Error::Internal(format!("Pipeline execution failed: {}", e)))?;

        // 4. Map back to Strategy-specific Pipeline record for compatibility
        let pipeline = Pipeline {
            id: pipeline_id,
            strategy_id: strategy.id.clone(),
            user_id: strategy.user_id.clone(),
            status: if result_ctx.aborted { 
                PipelineStatus::Cancelled { reason: "Aborted".to_string() } 
            } else { 
                PipelineStatus::Completed 
            },
            current_step: strategy.actions.len(), // Assume finished if generic pipeline finished
            step_results: Vec::new(), // We could populate this from result_ctx.trace if needed
            started_at: now,
            completed_at: Some(chrono::Utc::now().timestamp()),
        };

        Ok(pipeline)
    }
}
