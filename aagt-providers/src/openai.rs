//! OpenAI provider implementation
//!
//! Also compatible with OpenAI-compatible APIs like Groq, Mistral, etc.

use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures::{Stream, StreamExt};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};

use crate::{Error, Result, Message, StreamingChoice, StreamingResponse, ToolDefinition, Provider, HttpConfig};
use aagt_core::message::{Role, Content};

/// OpenAI API client
pub struct OpenAI {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl OpenAI {
    /// Create from API key
    pub fn new(api_key: impl Into<String>) -> Result<Self> {
        Self::with_base_url(api_key, "https://api.openai.com/v1")
    }

    /// Create from environment variable
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| Error::ProviderAuth("OPENAI_API_KEY not set".to_string()))?;
        Self::new(api_key)
    }

    /// Create with custom base URL (for compatible APIs)
    pub fn with_base_url(api_key: impl Into<String>, base_url: impl Into<String>) -> Result<Self> {
        let config = HttpConfig::default();
        let client = config.build_client()?;

        Ok(Self {
            client,
            api_key: api_key.into(),
            base_url: base_url.into(),
        })
    }

    /// Create for Groq
    pub fn groq(api_key: impl Into<String>) -> Result<Self> {
        Self::with_base_url(api_key, "https://api.groq.com/openai/v1")
    }

    /// Create for Mistral
    pub fn mistral(api_key: impl Into<String>) -> Result<Self> {
        Self::with_base_url(api_key, "https://api.mistral.ai/v1")
    }

    fn build_headers(&self) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", self.api_key))
                .map_err(|e| Error::Internal(e.to_string()))?,
        );
        Ok(headers)
    }
}

/// OpenAI chat completion request
#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<OpenAITool>,
    stream: bool,
}

#[derive(Debug, Serialize)]
struct OpenAIMessage {
    role: String,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAIToolCall>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct OpenAIToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: OpenAIFunction,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct OpenAIFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Serialize)]
struct OpenAITool {
    #[serde(rename = "type")]
    tool_type: String,
    function: OpenAIToolFunction,
}

#[derive(Debug, Serialize)]
struct OpenAIToolFunction {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

/// Streaming chunk from OpenAI
#[derive(Debug, Deserialize)]
struct StreamChunk {
    choices: Vec<StreamChoice>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: StreamDelta,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamDelta {
    content: Option<String>,
    tool_calls: Option<Vec<StreamToolCall>>,
}

#[derive(Debug, Deserialize)]
struct StreamToolCall {
    index: Option<usize>,
    id: Option<String>,
    function: Option<StreamFunction>,
}

#[derive(Debug, Deserialize)]
struct StreamFunction {
    name: Option<String>,
    arguments: Option<String>,
}

impl OpenAI {
    fn convert_messages(
        system_prompt: Option<&str>,
        messages: Vec<Message>,
    ) -> Vec<OpenAIMessage> {
        let mut result = Vec::with_capacity(messages.len() + 1);

        // Add system message if present
        if let Some(prompt) = system_prompt {
            result.push(OpenAIMessage {
                role: "system".to_string(),
                content: prompt.to_string(),
                name: None,
                tool_call_id: None,
                tool_calls: None,
            });
        }

        // Convert messages
        for msg in messages {
            let role = match msg.role {
                Role::System => "system",
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::Tool => "tool",
            };

            let mut content_string = String::new();
            let mut tool_calls = Vec::new();
            let mut tool_call_id = None;

            match msg.content {
                Content::Text(text) => content_string = text,
                Content::Parts(parts) => {
                    for part in parts {
                        match part {
                            aagt_core::message::ContentPart::Text { text } => content_string.push_str(&text),
                            aagt_core::message::ContentPart::ToolCall { id, name, arguments } => {
                                tool_calls.push(OpenAIToolCall {
                                    id,
                                    call_type: "function".to_string(),
                                    function: OpenAIFunction {
                                        name,
                                        arguments: arguments.to_string(),
                                    },
                                });
                            },
                            aagt_core::message::ContentPart::ToolResult { tool_call_id: id, content, .. } => {
                                // OpenAI expects one message per tool result
                                // If we have multiple results in one message (not standard AAGT but possible),
                                // we might need to split. But typically AAGT Message::tool_result is one result.
                                tool_call_id = Some(id);
                                content_string = content;
                            },
                            _ => {} // Images not supported in this basic impl yet
                        }
                    }
                }
            }

            result.push(OpenAIMessage {
                role: role.to_string(),
                content: content_string, // For tool results, this is the result. For assistant, this is thought/text.
                name: msg.name,
                tool_call_id,
                tool_calls: if tool_calls.is_empty() { None } else { Some(tool_calls) },
            });
        }

        result
    }

    fn convert_tools(tools: Vec<ToolDefinition>) -> Vec<OpenAITool> {
        tools
            .into_iter()
            .map(|t| OpenAITool {
                tool_type: "function".to_string(),
                function: OpenAIToolFunction {
                    name: t.name,
                    description: t.description,
                    parameters: t.parameters,
                },
            })
            .collect()
    }
}

#[async_trait]
impl Provider for OpenAI {
    async fn stream_completion(
        &self,
        model: &str,
        system_prompt: Option<&str>,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
        temperature: Option<f64>,
        max_tokens: Option<u64>,
        _extra_params: Option<serde_json::Value>,
    ) -> Result<StreamingResponse> {
        let request = ChatRequest {
            model: model.to_string(),
            messages: Self::convert_messages(system_prompt, messages),
            temperature,
            max_tokens,
            tools: Self::convert_tools(tools),
            stream: true,
        };

        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .headers(self.build_headers()?)
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(Error::ProviderApi(format!(
                "OpenAI API error {}: {}",
                status, text
            )));
        }

        // Parse SSE stream
        let stream = response.bytes_stream();
        let parsed_stream = parse_sse_stream(stream);

        Ok(StreamingResponse::from_stream(parsed_stream))
    }

    fn name(&self) -> &'static str {
        "openai"
    }
}

/// Parse Server-Sent Events stream from OpenAI
fn parse_sse_stream<S>(
    stream: S,
) -> impl Stream<Item = std::result::Result<StreamingChoice, Error>>
where
    S: Stream<Item = std::result::Result<bytes::Bytes, reqwest::Error>> + Send + Unpin + 'static,
{
    // Tool call accumulator state
    struct ToolCallState {
        id: String,
        name: String,
        arguments: String,
    }

    let buffer = String::new();
    let current_tool: Option<ToolCallState> = None;

    futures::stream::unfold(
        (stream, buffer, current_tool),
        move |(mut stream, mut buffer, mut current_tool)| async move {
            loop {
                // Try to extract a complete SSE message from buffer
                if let Some(pos) = buffer.find("\n\n") {
                    let message = buffer[..pos].to_string();
                    buffer = buffer[pos + 2..].to_string();

                    // Parse the SSE message
                    if let Some(data) = message.strip_prefix("data: ") {
                        if data.trim() == "[DONE]" {
                            return Some((Ok(StreamingChoice::Done), (stream, buffer, current_tool)));
                        }

                        match serde_json::from_str::<StreamChunk>(data) {
                            Ok(chunk) => {
                                if let Some(choice) = chunk.choices.first() {
                                    // Check for content
                                    if let Some(content) = &choice.delta.content {
                                        if !content.is_empty() {
                                            return Some((
                                                Ok(StreamingChoice::Message(content.clone())),
                                                (stream, buffer, current_tool),
                                            ));
                                        }
                                    }

                                    // Check for tool calls
                                    if let Some(tool_calls) = &choice.delta.tool_calls {
                                        for tc in tool_calls {
                                            // Start of new tool call
                                            if let Some(id) = &tc.id {
                                                current_tool = Some(ToolCallState {
                                                    id: id.clone(),
                                                    name: tc.function.as_ref()
                                                        .and_then(|f| f.name.clone())
                                                        .unwrap_or_default(),
                                                    arguments: String::new(),
                                                });
                                            }

                                            // Accumulate arguments
                                            if let Some(ref mut tool) = current_tool {
                                                if let Some(func) = &tc.function {
                                                    if let Some(args) = &func.arguments {
                                                        tool.arguments.push_str(args);
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    // Check if tool call is complete
                                    if choice.finish_reason.as_deref() == Some("tool_calls") {
                                        if let Some(tool) = current_tool.take() {
                                            let args: serde_json::Value = serde_json::from_str(&tool.arguments)
                                                .unwrap_or(serde_json::Value::Null);
                                            return Some((
                                                Ok(StreamingChoice::ToolCall {
                                                    id: tool.id,
                                                    name: tool.name,
                                                    arguments: args,
                                                }),
                                                (stream, buffer, None),
                                            ));
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!("Failed to parse SSE chunk: {}", e);
                            }
                        }
                    }
                    continue;
                }

                // Need more data
                match stream.next().await {
                    Some(Ok(bytes)) => {
                        match String::from_utf8(bytes.to_vec()) {
                            Ok(s) => buffer.push_str(&s),
                            Err(e) => {
                                return Some((
                                    Err(Error::StreamInterrupted(e.to_string())),
                                    (stream, buffer, current_tool),
                                ));
                            }
                        }
                    }
                    Some(Err(e)) => {
                        return Some((
                            Err(Error::Http(e)),
                            (stream, buffer, current_tool),
                        ));
                    }
                    None => {
                        // Stream ended
                        return None;
                    }
                }
            }
        },
    )
}

/// Common model constants
pub const GPT_4O: &str = "gpt-4o";
pub const GPT_4O_MINI: &str = "gpt-4o-mini";
pub const GPT_4_TURBO: &str = "gpt-4-turbo";
pub const GPT_35_TURBO: &str = "gpt-3.5-turbo";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_conversion() {
        let messages = vec![
            Message::user("Hello"),
            Message::assistant("Hi there!"),
        ];

        let converted = OpenAI::convert_messages(Some("Be helpful"), messages);
        
        assert_eq!(converted.len(), 3);
        assert_eq!(converted[0].role, "system");
        assert_eq!(converted[1].role, "user");
        assert_eq!(converted[2].role, "assistant");
    }
}
