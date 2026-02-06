//! Mock provider for testing

use async_trait::async_trait;

use crate::{Result, Message, StreamingResponse, ToolDefinition, Provider};
use aagt_core::agent::streaming::MockStreamBuilder;

/// A mock provider for testing
pub struct MockProvider {
    /// Response to return
    response: String,
}

impl MockProvider {
    /// Create a new mock provider with predefined response
    pub fn new(response: impl Into<String>) -> Self {
        Self {
            response: response.into(),
        }
    }
}

#[async_trait]
impl Provider for MockProvider {
    async fn stream_completion(
        &self,
        _model: &str,
        _system_prompt: Option<&str>,
        _messages: Vec<Message>,
        _tools: Vec<ToolDefinition>,
        _temperature: Option<f64>,
        _max_tokens: Option<u64>,
        _extra_params: Option<serde_json::Value>,
    ) -> Result<StreamingResponse> {
        // Split response into chunks for realistic streaming simulation
        let chunks: Vec<String> = self
            .response
            .chars()
            .collect::<Vec<_>>()
            .chunks(10)
            .map(|c| c.iter().collect())
            .collect();

        let mut builder = MockStreamBuilder::new();
        for chunk in chunks {
            builder = builder.message(chunk);
        }
        builder = builder.done();

        Ok(builder.build())
    }

    fn name(&self) -> &'static str {
        "mock"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_provider() {
        let provider = MockProvider::new("Hello, world!");
        let stream = provider
            .stream_completion(
                "test",
                None,
                vec![Message::user("Hi")],
                vec![],
                None,
                None,
                None,
            )
            .await
            .expect("should succeed");

        let text = stream.collect_text().await.expect("collect should succeed");
        assert_eq!(text, "Hello, world!");
    }
}

