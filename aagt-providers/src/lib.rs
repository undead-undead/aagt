//! # AAGT Providers
//!
//! LLM provider implementations for the AAGT framework.

#![warn(missing_docs)]

// Re-export core types for convenience
pub use aagt_core::error::{Error, Result};
pub use aagt_core::message::Message;
pub use aagt_core::provider::Provider;
pub use aagt_core::streaming::{StreamingChoice, StreamingResponse};
pub use aagt_core::tool::ToolDefinition;

pub mod mock;

#[cfg(feature = "openai")]
pub mod openai;

#[cfg(feature = "anthropic")]
pub mod anthropic;

#[cfg(feature = "gemini")]
pub mod gemini;

#[cfg(feature = "deepseek")]
pub mod deepseek;

#[cfg(feature = "openrouter")]
pub mod openrouter;

/// HTTP client configuration
#[derive(Clone)]
pub struct HttpConfig {
    /// Request timeout in seconds
    pub timeout_secs: u64,
    /// Connection pool idle timeout
    pub pool_idle_timeout_secs: u64,
    /// Max idle connections per host
    pub pool_max_idle_per_host: usize,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            timeout_secs: 60,
            pool_idle_timeout_secs: 90,
            pool_max_idle_per_host: 32,
        }
    }
}

impl HttpConfig {
    /// Build a reqwest client
    pub fn build_client(&self) -> Result<reqwest::Client> {
        use std::time::Duration;

        reqwest::Client::builder()
            .timeout(Duration::from_secs(self.timeout_secs))
            .pool_idle_timeout(Duration::from_secs(self.pool_idle_timeout_secs))
            .pool_max_idle_per_host(self.pool_max_idle_per_host)
            .build()
            .map_err(|e| Error::Internal(e.to_string()))
    }
}
