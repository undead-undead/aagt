use std::sync::Arc;
use tokio::sync::Mutex;
use std::time::{Duration, Instant};
use async_trait::async_trait;
use tracing::{info, warn};

use crate::error::Result;
use crate::agent::message::Message;
use crate::agent::provider::Provider;
use crate::skills::tool::ToolDefinition;
use crate::agent::streaming::StreamingResponse;

/// Configuration for the Circuit Breaker
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Failure threshold before opening the circuit
    pub failure_threshold: u32,
    /// Duration to wait before attempting recovery (Half-Open)
    pub reset_timeout: Duration,
    /// Maximum request duration before considering it a failure (Timeout)
    pub request_timeout: Duration,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 3,
            reset_timeout: Duration::from_secs(60),
            request_timeout: Duration::from_secs(30),
        }
    }
}

/// State of the Circuit Breaker
#[derive(Debug, Clone, PartialEq)]
enum CircuitState {
    Closed,   // Normal operation
    Open,     // Failing, use fallback
    HalfOpen, // Recovering, test primary
}

/// A provider that wraps a primary and a fallback provider with circuit breaker logic
pub struct ResilientProvider<P: Provider, F: Provider> {
    primary: Arc<P>,
    fallback: Arc<F>,
    config: CircuitBreakerConfig,
    state: Arc<Mutex<CircuitStateInternal>>,
}

struct CircuitStateInternal {
    state: CircuitState,
    failures: u32,
    last_failure_time: Option<Instant>,
}

impl<P: Provider, F: Provider> ResilientProvider<P, F> {
    pub fn new(primary: P, fallback: F, config: CircuitBreakerConfig) -> Self {
        Self {
            primary: Arc::new(primary),
            fallback: Arc::new(fallback),
            config,
            state: Arc::new(Mutex::new(CircuitStateInternal {
                state: CircuitState::Closed,
                failures: 0,
                last_failure_time: None,
            })),
        }
    }

    async fn check_state(&self) -> CircuitState {
        let mut router = self.state.lock().await;
        
        match router.state {
            CircuitState::Open => {
                if let Some(last_failure) = router.last_failure_time {
                    if last_failure.elapsed() > self.config.reset_timeout {
                        info!("Circuit Breaker: Reset timeout passed, switching to Half-Open");
                        router.state = CircuitState::HalfOpen;
                        return CircuitState::HalfOpen;
                    }
                }
                CircuitState::Open
            }
            _ => router.state.clone(),
        }
    }

    async fn report_success(&self) {
        let mut router = self.state.lock().await;
        if router.state == CircuitState::HalfOpen {
            info!("Circuit Breaker: Half-Open success, closing circuit (Back to Normal)");
            router.state = CircuitState::Closed;
            router.failures = 0;
            router.last_failure_time = None;
        } else if router.state == CircuitState::Closed {
             router.failures = 0;
        }
    }

    async fn report_failure(&self) {
        let mut router = self.state.lock().await;
        router.failures += 1;
        router.last_failure_time = Some(Instant::now());

        if router.state == CircuitState::Closed && router.failures >= self.config.failure_threshold {
            warn!("Circuit Breaker: Failure threshold reached, OPENING circuit (Switching to Fallback)");
            router.state = CircuitState::Open;
        } else if router.state == CircuitState::HalfOpen {
            warn!("Circuit Breaker: Half-Open failure, re-opening circuit");
            router.state = CircuitState::Open;
        }
    }
}

#[async_trait]
impl<P: Provider, F: Provider> Provider for ResilientProvider<P, F> {
    fn name(&self) -> &'static str {
        "resilient-provider"
    }

    async fn stream_completion(
        &self,
        model: &str,
        system_prompt: Option<&str>,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
        temperature: Option<f64>,
        max_tokens: Option<u64>,
        extra_params: Option<serde_json::Value>,
    ) -> Result<StreamingResponse> {
        let state = self.check_state().await;
        
        // Decide which provider to use
        let use_primary = match state {
            CircuitState::Closed => true,
            CircuitState::HalfOpen => true, // Try one request
            CircuitState::Open => false,
        };

        if use_primary {
            // Attempt Primary with Timeout
            match tokio::time::timeout(
                self.config.request_timeout,
                self.primary.stream_completion(
                    model, system_prompt, messages.clone(), tools.clone(), temperature, max_tokens, extra_params.clone()
                )
            ).await {
                Ok(Ok(response)) => {
                    self.report_success().await;
                    return Ok(response);
                }
                Ok(Err(e)) => {
                    warn!("Primary provider failed: {}", e);
                    self.report_failure().await;
                    // Fallthrough to fallback
                }
                Err(_) => {
                    warn!("Primary provider timed out (> {:?})", self.config.request_timeout);
                    self.report_failure().await;
                    // Fallthrough to fallback
                }
            }
        }

        // Fallback Logic
        info!("Using Fallback Provider: {}", self.fallback.name());
        
        // Note: We might want a different model name for fallback, but for now we pass the same Request.
        // In a real scenario, ResilientProvider might need a mapping or the fallback provider ignores the model param.
        self.fallback.stream_completion(
             model, system_prompt, messages, tools, temperature, max_tokens, extra_params
        ).await
    }
}
