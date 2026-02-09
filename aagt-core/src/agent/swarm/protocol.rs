use crate::agent::swarm::manifest::AgentManifest;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum SwarmMessage {
    /// Announce presence to the network
    Announcement(AgentManifest),

    /// Request help with a task
    TaskRequest {
        request_id: String,
        requester_id: String,
        task_description: String,
        required_capabilities: Vec<String>,
        timeout_ms: u64,
    },

    /// Bid to perform a task
    Bid {
        request_id: String,
        bidder_id: String,
        bidder_name: String,
        estimated_time_ms: u64,
        confidence: f32,  // 0.0 - 1.0
        proposal: String, // "I will use Python to..."
    },

    /// Assign a task to a specific bidder
    TaskAssignment {
        request_id: String,
        assigned_to: String,
        task_context: String, // Additional context/instructions
    },

    /// Return the result of a task
    Result {
        request_id: String,
        performer_id: String,
        output: String,
        success: bool,
    },
}

impl SwarmMessage {
    pub fn new_request(requester_id: &str, task: &str, caps: Vec<String>) -> Self {
        SwarmMessage::TaskRequest {
            request_id: Uuid::new_v4().to_string(),
            requester_id: requester_id.to_string(),
            task_description: task.to_string(),
            required_capabilities: caps,
            timeout_ms: 30000,
        }
    }

    pub fn request_id(&self) -> Option<&str> {
        match self {
            SwarmMessage::TaskRequest { request_id, .. } => Some(request_id),
            SwarmMessage::Bid { request_id, .. } => Some(request_id),
            SwarmMessage::TaskAssignment { request_id, .. } => Some(request_id),
            SwarmMessage::Result { request_id, .. } => Some(request_id),
            SwarmMessage::Announcement(_) => None,
        }
    }
}
