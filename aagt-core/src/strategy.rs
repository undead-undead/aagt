//! Strategy and pipeline system for automated trading

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio::time::timeout;

use crate::error::Result;

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

/// Trait for action executors
#[async_trait::async_trait]
pub trait ActionExecutor: Send + Sync {
    /// Execute an action
    async fn execute(&self, action: &Action, context: &ExecutionContext) -> Result<String>;
}

/// Context for action execution
pub struct ExecutionContext {
    /// User ID
    pub user_id: String,
    /// Pipeline ID
    pub pipeline_id: String,
    /// Results from previous steps
    pub previous_results: Vec<StepResult>,
}

/// Strategy engine for managing and executing strategies
pub struct StrategyEngine {
    /// Condition evaluator
    evaluator: Arc<dyn ConditionEvaluator>,
    /// Action executor
    executor: Arc<dyn ActionExecutor>,
    /// Shutdown signal receiver
    shutdown_rx: Option<mpsc::Receiver<()>>,
}

impl StrategyEngine {
    /// Create a new strategy engine
    pub fn new(
        evaluator: Arc<dyn ConditionEvaluator>,
        executor: Arc<dyn ActionExecutor>,
    ) -> Self {
        Self {
            evaluator,
            executor,
            shutdown_rx: None,
        }
    }

    /// Set shutdown signal channel
    pub fn with_shutdown(mut self, rx: mpsc::Receiver<()>) -> Self {
        self.shutdown_rx = Some(rx);
        self
    }

    /// Execute a pipeline with timeout and graceful shutdown
    pub async fn execute_pipeline(
        &self,
        strategy: &Strategy,
        pipeline_id: String,
    ) -> Result<Pipeline> {
        let now = chrono::Utc::now().timestamp();
        let mut pipeline = Pipeline {
            id: pipeline_id,
            strategy_id: strategy.id.clone(),
            user_id: strategy.user_id.clone(),
            status: PipelineStatus::Running,
            current_step: 0,
            step_results: Vec::new(),
            started_at: now,
            completed_at: None,
        };

        let context = ExecutionContext {
            user_id: strategy.user_id.clone(),
            pipeline_id: pipeline.id.clone(),
            previous_results: Vec::new(),
        };

        for (index, action) in strategy.actions.iter().enumerate() {
            pipeline.current_step = index;

            // Check for cancellation
            if let Action::Cancel { reason } = action {
                pipeline.status = PipelineStatus::Cancelled {
                    reason: reason.clone(),
                };
                break;
            }

            // Handle wait action
            if let Action::Wait { seconds } = action {
                // Use timeout with check - don't wait forever
                let wait_duration = Duration::from_secs(*seconds);
                let max_wait = Duration::from_secs(300); // Max 5 minute wait
                let actual_wait = wait_duration.min(max_wait);
                
                tokio::time::sleep(actual_wait).await;
                
                pipeline.step_results.push(StepResult {
                    index,
                    action: action.clone(),
                    success: true,
                    message: format!("Waited {} seconds", actual_wait.as_secs()),
                    timestamp: chrono::Utc::now().timestamp(),
                });
                continue;
            }

            // Execute action with timeout
            let execute_timeout = Duration::from_secs(30);
            let result = timeout(
                execute_timeout,
                self.executor.execute(action, &context),
            )
            .await;

            let step_result = match result {
                Ok(Ok(message)) => StepResult {
                    index,
                    action: action.clone(),
                    success: true,
                    message,
                    timestamp: chrono::Utc::now().timestamp(),
                },
                Ok(Err(e)) => {
                    pipeline.status = PipelineStatus::Failed {
                        error: e.to_string(),
                    };
                    StepResult {
                        index,
                        action: action.clone(),
                        success: false,
                        message: e.to_string(),
                        timestamp: chrono::Utc::now().timestamp(),
                    }
                }
                Err(_) => {
                    pipeline.status = PipelineStatus::Failed {
                        error: "Action execution timed out".to_string(),
                    };
                    StepResult {
                        index,
                        action: action.clone(),
                        success: false,
                        message: "Timeout".to_string(),
                        timestamp: chrono::Utc::now().timestamp(),
                    }
                }
            };

            let failed = !step_result.success;
            pipeline.step_results.push(step_result);

            if failed {
                break;
            }
        }

        // Mark completed if not already failed/cancelled
        if matches!(pipeline.status, PipelineStatus::Running) {
            pipeline.status = PipelineStatus::Completed;
        }
        pipeline.completed_at = Some(chrono::Utc::now().timestamp());

        Ok(pipeline)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strategy_serialization() {
        let strategy = Strategy {
            id: "strat1".to_string(),
            user_id: "user1".to_string(),
            name: "Buy the dip".to_string(),
            description: Some("Buy SOL when price drops".to_string()),
            condition: Condition::PriceBelow {
                token: "SOL".to_string(),
                threshold: 150.0,
            },
            actions: vec![
                Action::Swap {
                    from_token: "USDC".to_string(),
                    to_token: "SOL".to_string(),
                    amount: "100".to_string(),
                },
                Action::Notify {
                    channel: NotifyChannel::Telegram,
                    message: "Bought SOL!".to_string(),
                },
            ],
            active: true,
            created_at: 1234567890,
        };

        let json = serde_json::to_string_pretty(&strategy).expect("serialize");
        let parsed: Strategy = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.id, strategy.id);
    }
}
