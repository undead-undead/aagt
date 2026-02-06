//! Background scheduler for proactive agent tasks
//!
//! Enables agents to handle periodic tasks and timed events using tokio-cron-scheduler.

use std::sync::Weak;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use dashmap::DashMap;
use tracing::{info, error, debug};
use tokio_cron_scheduler::{Job, JobScheduler};

use crate::error::{Error, Result};
use crate::agent::multi_agent::{Coordinator, AgentRole};

/// Schedule for a job
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum JobSchedule {
    /// One-shot at absolute time
    #[serde(rename_all = "camelCase")]
    At { 
        #[serde(with = "chrono::serde::ts_seconds")]
        at: DateTime<Utc> 
    },
    /// Recurring interval
    #[serde(rename_all = "camelCase")]
    Every { 
        interval_secs: u64 
    },
    /// Cron expression
    #[serde(rename_all = "camelCase")]
    Cron { 
        expr: String 
    },
}

/// Payload for a job
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum JobPayload {
    /// Run an agent process with a prompt
    #[serde(rename_all = "camelCase")]
    AgentTurn {
        role: AgentRole,
        prompt: String,
    },
    /// Generate a summary for a document and store it
    #[serde(rename_all = "camelCase")]
    SummarizeDoc {
        collection: String,
        path: String,
        content: String,
    },
}

/// A scheduled job (Metadata for listing/canceling)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    /// Unique ID (Matches the one in tokio-cron-scheduler)
    pub id: Uuid,
    /// Human-readable name
    pub name: String,
    /// Schedule definition
    pub schedule: JobSchedule,
    /// Payload to execute
    pub payload: JobPayload,
    /// Whether the job is enabled
    pub enabled: bool,
}

/// Scheduler service wrapping tokio-cron-scheduler
pub struct Scheduler {
    /// Registered jobs metadata
    jobs: DashMap<Uuid, CronJob>,
    /// The underlying scheduler
    scheduler: tokio::sync::Mutex<JobScheduler>,
    /// Weak reference to coordinator for execution
    coordinator: Weak<Coordinator>,
}

impl Scheduler {
    /// Create a new scheduler
    pub async fn new(coordinator: Weak<Coordinator>) -> Self {
        let scheduler = JobScheduler::new().await.expect("Failed to initialize JobScheduler");
        Self {
            jobs: DashMap::new(),
            scheduler: tokio::sync::Mutex::new(scheduler),
            coordinator,
        }
    }

    /// Add a job
    pub async fn add_job(&self, name: String, schedule: JobSchedule, payload: JobPayload) -> Result<Uuid> {
        let coordinator_weak = self.coordinator.clone();
        let payload_clone = payload.clone();
        let name_clone = name.clone();
        
        // 1. Create the job based on schedule type
        let job = match &schedule {
            JobSchedule::At { at } => {
                let now = Utc::now();
                let duration = at.signed_duration_since(now).to_std()
                    .map_err(|_| Error::agent_config("Scheduled time is in the past"))?;
                
                // One-shot job using a duration
                Job::new_one_shot_async(duration, move |_uuid, _l| {
                    let coordinator_weak = coordinator_weak.clone();
                    let payload = payload_clone.clone();
                    let name = name_clone.clone();
                    Box::pin(async move {
                        if let Err(e) = Self::execute_payload(&coordinator_weak, &name, payload).await {
                            error!("Failed to execute one-shot job {}: {}", name, e);
                        }
                    })
                }).map_err(|e| Error::Internal(format!("Failed to create one-shot job: {}", e)))?
            }
            JobSchedule::Every { interval_secs } => {
                let duration = std::time::Duration::from_secs(*interval_secs);
                Job::new_repeated_async(duration, move |_uuid, _l| {
                    let coordinator_weak = coordinator_weak.clone();
                    let payload = payload_clone.clone();
                    let name = name_clone.clone();
                    Box::pin(async move {
                        if let Err(e) = Self::execute_payload(&coordinator_weak, &name, payload).await {
                            error!("Failed to execute repeated job {}: {}", name, e);
                        }
                    })
                }).map_err(|e| Error::Internal(format!("Failed to create repeated job: {}", e)))?
            }
            JobSchedule::Cron { expr } => {
                Job::new_async(expr.as_str(), move |_uuid, _l| {
                    let coordinator_weak = coordinator_weak.clone();
                    let payload = payload_clone.clone();
                    let name = name_clone.clone();
                    Box::pin(async move {
                        if let Err(e) = Self::execute_payload(&coordinator_weak, &name, payload).await {
                            error!("Failed to execute cron job {}: {}", name, e);
                        }
                    })
                }).map_err(|e| Error::Internal(format!("Failed to create cron job: {}", e)))?
            }
        };

        // 2. Add to underlying scheduler
        let sched = self.scheduler.lock().await;
        let id = sched.add(job).await
            .map_err(|e| Error::Internal(format!("Failed to add job to scheduler: {}", e)))?;
        
        // 3. Store metadata
        self.jobs.insert(id, CronJob {
            id,
            name,
            schedule,
            payload,
            enabled: true,
        });
        
        Ok(id)
    }

    /// List all jobs
    pub fn list_jobs(&self) -> Vec<CronJob> {
        self.jobs.iter().map(|r| r.value().clone()).collect()
    }

    /// Remove a job
    pub async fn remove_job(&self, id: Uuid) -> Result<bool> {
        let sched = self.scheduler.lock().await;
        sched.remove(&id).await
            .map_err(|e| Error::Internal(format!("Failed to remove job: {}", e)))?;
        Ok(self.jobs.remove(&id).is_some())
    }

    /// Start the scheduler loop
    pub async fn run(&self) {
        let sched = self.scheduler.lock().await;
        if let Err(e) = sched.start().await {
            error!("Failed to start scheduler: {}", e);
        }
    }

    async fn execute_payload(coordinator_weak: &Weak<Coordinator>, name: &str, payload: JobPayload) -> Result<()> {
        info!("Executing scheduled job: {}", name);
        
        let coordinator = coordinator_weak.upgrade()
            .ok_or_else(|| Error::AgentCoordination("Coordinator dropped".to_string()))?;
            
        match payload {
            JobPayload::AgentTurn { role, prompt } => {
                if let Some(agent) = coordinator.get(&role) {
                    debug!("Triggering proactive process for agent {:?}", role);
                    agent.process(&prompt).await?;
                } else {
                    return Err(Error::AgentCoordination(format!("Target agent {:?} not found", role)));
                }
            }
            JobPayload::SummarizeDoc { collection, path, content } => {
                // Use Assistant or Researcher to summarize
                let agent = coordinator.get(&AgentRole::Assistant)
                    .or_else(|| coordinator.get(&AgentRole::Researcher))
                    .ok_or_else(|| Error::AgentCoordination("No agent available for summarization".to_string()))?;
                
                let prompt = format!(
                    "Summarize the following document in about 200 words. Focus on core concepts and key information.\n\nDocument Content:\n{}", 
                    content
                );
                
                debug!("Generating summary for {}/{}", collection, path);
                let summary = agent.process(&prompt).await?;
                
                // We need to update the summary in memory. 
                // Since QmdMemory is usually the LTM, we'll try to update it through an agent if possible,
                // or we might need a more direct way if the Coordinator doesn't expose memory.
                // For now, we'll look for a way to get the memory from the agent.
                // NOTE: This assumes the agent's process call doesn't already do this.
                // In our " Tiered RAG" design, the worker does the update.
                
                // Since we don't have a clean way to get Memory from the MultiAgent trait right now,
                // we'll use a placeholder/TODO or fix the trait.
                // Actually, let's assume the coordinator might have a way or we add it.
                
                info!("Summary generated for {}/{} ({} chars)", collection, path, summary.len());
                
                if let Some(memory) = coordinator.memory.get() {
                    memory.update_summary(&collection, &path, &summary).await?;
                    info!("Successfully updated summary in memory for {}/{}", collection, path);
                } else {
                    tracing::warn!("Generated summary but no shared memory found in coordinator");
                }
            }
        }
        
        Ok(())
    }
}
