use crate::agent::multi_agent::AgentRole;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Status of an agent in the swarm
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    /// Ready to accept tasks
    Idle,
    /// Currently processing a task
    Busy,
    /// Temporarily unavailable (e.g. recovering)
    Unavailable,
    /// Offline (should be pruned from discovery)
    Offline,
}

/// Manifest declaring an agent's identity and capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentManifest {
    /// Unique ID of the agent instance
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Primary role in the swarm
    pub role: AgentRole,
    /// List of capabilities (tool names, skill tags)
    pub capabilities: HashSet<String>,
    /// Current status
    pub status: AgentStatus,
    /// Address for direct communication (optional, for p2p)
    pub address: Option<String>,
    /// Last heartbeat timestamp (unix millis)
    pub last_seen: u64,
    /// Standard Operating Procedure / Mission Statement for logic guidance
    pub sop: Option<String>,
}

impl AgentManifest {
    pub fn new(id: impl Into<String>, name: impl Into<String>, role: AgentRole) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            role,
            capabilities: HashSet::new(),
            status: AgentStatus::Idle,
            address: None,
            last_seen: chrono::Utc::now().timestamp_millis() as u64,
            sop: None,
        }
    }

    pub fn with_capability(mut self, cap: impl Into<String>) -> Self {
        self.capabilities.insert(cap.into());
        self
    }

    pub fn with_capabilities<I, S>(mut self, caps: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for cap in caps {
            self.capabilities.insert(cap.into());
        }
        self
    }

    pub fn update_status(&mut self, status: AgentStatus) {
        self.status = status;
        self.touch();
    }

    pub fn touch(&mut self) {
        self.last_seen = chrono::Utc::now().timestamp_millis() as u64;
    }

    pub fn is_stale(&self, timeout_ms: u64) -> bool {
        let now = chrono::Utc::now().timestamp_millis() as u64;
        now.saturating_sub(self.last_seen) > timeout_ms
    }
}
