//! Mock provider for testing

use async_trait::async_trait;

use crate::{Result, Message, StreamingResponse, ToolDefinition, Provider};
use aagt_core::agent::streaming::MockStreamBuilder;

/// A mock provider for testing
pub struct MockProvider {
    /// Response to return
    response: String,
    /// Optional tool calls to inject
    tool_calls: Vec<(String, String, serde_json::Value)>,
}

impl MockProvider {
    /// Create a new mock provider with predefined response
    pub fn new(response: impl Into<String>) -> Self {
        Self {
            response: response.into(),
            tool_calls: Vec::new(),
        }
    }

    /// Create a mock provider that returns tool calls
    pub fn with_tool_calls(
        response: impl Into<String>,
        tool_calls: Vec<(String, String, serde_json::Value)>,
    ) -> Self {
        Self {
            response: response.into(),
            tool_calls,
        }
    }
}

#[async_trait]
impl Provider for MockProvider {
    async fn stream_completion(
        &self,
        request: aagt_core::agent::provider::ChatRequest,
    ) -> Result<StreamingResponse> {
        // Simple logic to avoid infinite loops: 
        // Only return tool calls if the last message isn't already a tool result.
        let is_last_tool_result = request.messages.last().map(|m| m.role == aagt_core::agent::message::Role::Tool).unwrap_or(false);

        // Split response into chunks for realistic streaming simulation
        let chunks: Vec<String> = self
            .response
            .chars()
            .collect::<Vec<_>>()
            .chunks(10)
            .map(|c| c.iter().collect())
            .collect();

        let mut builder = MockStreamBuilder::new();
        
        if is_last_tool_result {
            builder = builder.message("I have processed the tool result.");
        } else {
            for chunk in chunks {
                builder = builder.message(chunk);
            }

            for (id, name, args) in &self.tool_calls {
                builder = builder.tool_call(id, name, args.clone());
            }
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
            .stream_completion(aagt_core::agent::provider::ChatRequest {
                model: "test".to_string(),
                messages: vec![Message::user("Hi")],
                ..Default::default()
            })
            .await
            .expect("should succeed");

        let text = stream.collect_text().await.expect("collect should succeed");
        assert_eq!(text, "Hello, world!");
    }
}

