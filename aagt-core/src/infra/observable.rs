//! Observability infrastructure for agents
//!
//! Provides traits and helpers to observe agent events for logging, 
//! UI updates, or remote monitoring.

use async_trait::async_trait;
use crate::agent::core::AgentEvent;

/// Trait for observing agent events
#[async_trait]
pub trait AgentObserver: Send + Sync {
    /// Handle an agent event
    async fn on_event(&self, event: &AgentEvent) -> crate::error::Result<()>;
}

/// A dispatcher that forwards events from a broadcast channel to multiple observers
pub struct EventDispatcher {
    observers: Vec<Box<dyn AgentObserver>>,
}

impl EventDispatcher {
    pub fn new() -> Self {
        Self { observers: Vec::new() }
    }

    pub fn add_observer(&mut self, observer: Box<dyn AgentObserver>) {
        self.observers.push(observer);
    }

    pub async fn dispatch(&self, event: &AgentEvent) {
        for observer in &self.observers {
            if let Err(e) = observer.on_event(event).await {
                tracing::error!("Observer failed to handle event: {}", e);
            }
        }
    }
}
