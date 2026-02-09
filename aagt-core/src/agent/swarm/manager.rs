use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};
use tracing::info;
use crate::error::Result;
use crate::agent::swarm::manifest::{AgentManifest, AgentStatus};
use crate::agent::swarm::discovery::Discovery;
use crate::agent::swarm::protocol::SwarmMessage;

/// Events/Commands from SwarmManager to the Agent
#[derive(Debug, Clone)]
pub enum SwarmEvent {
    /// Request the agent to execute a task
    ExecuteTask {
        request_id: String,
        task: String,
        context: String,
    },
    /// Notification that a delegated task completed
    TaskResult {
        request_id: String,
        result: String,
        success: bool,
    },
}

/// State of a request initiated by this agent
struct PendingRequest {
    task: String,
    created_at: u64,
    bids: Vec<SwarmMessage>, // Store Bid messages
}

/// Manages swarm participation for an agent
pub struct SwarmManager {
    /// Identity of this agent
    identity: AgentManifest,
    /// Discovery mechanism
    discovery: Arc<dyn Discovery>,
    /// Channel for sending messages to the network/swarm
    outbox: broadcast::Sender<SwarmMessage>,
    /// Channel for receiving messages from network
    inbox: broadcast::Receiver<SwarmMessage>,
    
    /// Channel to send events to the managing Agent
    agent_tx: mpsc::Sender<SwarmEvent>,
    
    /// Channel receiver for the Agent to pick up (initially Some, taken by Agent)
    agent_rx: Option<mpsc::Receiver<SwarmEvent>>,
    
    /// Requests I have sent and am waiting for bids on
    /// Key: request_id
    pending_requests: HashMap<String, PendingRequest>,
    
    /// Tasks I effectively have assigned to me (but maybe not yet started by Agent)
    active_tasks: HashMap<String, String>, // request_id -> task_desc
}

impl SwarmManager {
    pub fn new(
        identity: AgentManifest,
        discovery: Arc<dyn Discovery>,
        bus: broadcast::Sender<SwarmMessage>,
    ) -> Self {
        let inbox = bus.subscribe();
        let (agent_tx, agent_rx) = mpsc::channel(100);
        
        Self {
            identity,
            discovery,
            outbox: bus,
            inbox,
            agent_tx,
            agent_rx: Some(agent_rx),
            pending_requests: HashMap::new(),
            active_tasks: HashMap::new(),
        }
    }
    
    /// Allow the owning Agent to take the command receiver
    pub fn take_command_receiver(&mut self) -> Option<mpsc::Receiver<SwarmEvent>> {
        self.agent_rx.take()
    }

    /// Update the agent's SOP in the manifest
    pub fn set_sop(&mut self, sop: Option<String>) {
        self.identity.sop = sop;
    }

    pub fn discovery(&self) -> Arc<dyn Discovery> {
        self.discovery.clone()
    }

    /// Announce presence to the swarm
    pub async fn announce(&self) -> Result<()> {
        self.discovery.register(self.identity.clone()).await?;
        let msg = SwarmMessage::Announcement(self.identity.clone());
        let _ = self.outbox.send(msg);
        info!("Swarm: Announced presence as {}", self.identity.name);
        Ok(())
    }

    /// Find best agent for a task (via Discovery)
    pub async fn find_best_agent(&self, required_capability: &str) -> Result<Option<AgentManifest>> {
        let candidates = self.discovery.find_by_capability(required_capability).await?;
        let best = candidates.iter()
            .find(|a| a.status == AgentStatus::Idle)
            .or(candidates.first())
            .cloned();
        Ok(best)
    }

    /// Broadcast a task request and track it
    pub async fn broadcast_request(&mut self, task: &str, capabilities: Vec<String>) -> Result<String> {
        let msg = SwarmMessage::new_request(&self.identity.id, task, capabilities);
        
        let request_id = match &msg {
            SwarmMessage::TaskRequest { request_id, .. } => request_id.clone(),
            _ => unreachable!(),
        };
        
        // Track pending request
        self.pending_requests.insert(request_id.clone(), PendingRequest {
            task: task.to_string(),
            created_at: 0, // TODO: use chrono or timestamp
            bids: Vec::new(),
        });
        
        let _ = self.outbox.send(msg);
        info!("Swarm: Broadcasted request {} for task '{}'", request_id, task);
        Ok(request_id)
    }
    
    /// Accept a bid and assign the task
    pub async fn accept_bid(&mut self, request_id: &str, bidder_id: &str) -> Result<()> {
        if let Some(req) = self.pending_requests.get(request_id) {
            info!("Swarm: Accepting bid from {} for request {}", bidder_id, request_id);
            let msg = SwarmMessage::TaskAssignment {
                request_id: request_id.to_string(),
                assigned_to: bidder_id.to_string(),
                task_context: req.task.clone(),
            };
            let _ = self.outbox.send(msg);
        }
        Ok(())
    }

    /// Process incoming swarm messages
    pub async fn process_inbox(&mut self) -> Result<()> {
        while let Ok(msg) = self.inbox.try_recv() {
            self.handle_message(msg).await?;
        }
        Ok(())
    }
    
    async fn handle_message(&mut self, msg: SwarmMessage) -> Result<()> {
        match msg {
            SwarmMessage::TaskRequest { request_id, requester_id, task_description, required_capabilities, .. } => {
                // Ignore own requests
                if requester_id == self.identity.id {
                    return Ok(());
                }
                
                // Check capabilities
                let can_handle = required_capabilities.iter().all(|cap| self.identity.capabilities.contains(cap));
                
                if can_handle && self.identity.status == AgentStatus::Idle {
                    info!("Swarm: Sending bid for request {}", request_id);
                    let bid = SwarmMessage::Bid {
                        request_id: request_id.clone(),
                        bidder_id: self.identity.id.clone(),
                        bidder_name: self.identity.name.clone(),
                        estimated_time_ms: 1000,
                        confidence: 0.9,
                        proposal: format!("I can help with '{}'", task_description),
                    };
                    let _ = self.outbox.send(bid);
                }
            }
            
            SwarmMessage::Bid { request_id, bidder_id, .. } => {
                // If this is for one of my requests, store it
                if let Some(req) = self.pending_requests.get_mut(&request_id) {
                    info!("Swarm: Received bid from {} for request {}", bidder_id, request_id);
                    req.bids.push(SwarmMessage::Bid {
                         request_id: request_id.clone(),
                         bidder_id: bidder_id.clone(),
                         bidder_name: "".to_string(), // we don't need full clone if we store msg, but for now ok
                         estimated_time_ms: 0,
                         confidence: 0.0,
                         proposal: "".to_string(),
                    });
                    // For MVP: Auto-accept the first bid
                    // In real system, we might wait for more bids
                    self.accept_bid(&request_id, &bidder_id).await?;
                }
            }
            
            SwarmMessage::TaskAssignment { request_id, assigned_to, task_context } => {
                // If assigned to me
                if assigned_to == self.identity.id {
                    info!("Swarm: Application accepted! Assigned task: {}", request_id);
                    self.active_tasks.insert(request_id.clone(), task_context.clone());
                    
                    // Notify Agent to execute
                    let _ = self.agent_tx.send(SwarmEvent::ExecuteTask {
                        request_id,
                        task: task_context.clone(),
                        context: "".to_string(),
                    }).await;
                }
            }
            
            SwarmMessage::Result { request_id, performer_id, output, success } => {
                // If this is for my request
                if self.pending_requests.contains_key(&request_id) {
                    info!("Swarm: Received result from {} for request {}: {}", performer_id, request_id, success);
                    
                    // Notify Agent
                    let _ = self.agent_tx.send(SwarmEvent::TaskResult {
                        request_id: request_id.clone(),
                        result: output,
                        success,
                    }).await;
                    
                    // Cleanup
                    self.pending_requests.remove(&request_id);
                }
            }
            
            _ => {}
        }
        Ok(())
    }
    
    /// Send a result back to the network (called by Agent when done)
    pub async fn send_result(&mut self, request_id: &str, output: String, success: bool) -> Result<()> {
        if self.active_tasks.remove(request_id).is_some() {
            let msg = SwarmMessage::Result {
                request_id: request_id.to_string(),
                performer_id: self.identity.id.clone(),
                output,
                success,
            };
            let _ = self.outbox.send(msg);
            info!("Swarm: Sent result for task {}", request_id);
        }
        Ok(())
    }
}
