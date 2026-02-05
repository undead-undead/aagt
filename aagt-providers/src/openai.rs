//! OpenAI provider implementation
//!
//! Also compatible with OpenAI-compatible APIs like Groq, Mistral, etc.



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
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
    stream: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ResponseFormat {
    #[serde(rename = "type")]
    format_type: String,
}

#[derive(Debug, Serialize)]
struct OpenAIMessage {
    role: String,
    content: serde_json::Value,
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
                content: serde_json::Value::String(prompt.to_string()),
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

            let mut tool_calls = Vec::new();
            let mut tool_call_id = None;
            let final_content: serde_json::Value;

            match msg.content {
                Content::Text(text) => {
                    final_content = serde_json::Value::String(text);
                },
                Content::Parts(parts) => {
                    let mut json_parts = Vec::new();
                    let mut text_acc = String::new();
                    
                    for part in parts {
                        match part {
                            aagt_core::message::ContentPart::Text { text } => {
                                text_acc.push_str(&text);
                                json_parts.push(serde_json::json!({
                                    "type": "text",
                                    "text": text
                                }));
                            },
                            aagt_core::message::ContentPart::Image { source } => {
                                // Fix #8: Support Images (Url and Base64)
                                let url = match source {
                                    aagt_core::message::ImageSource::Url { url } => url,
                                    aagt_core::message::ImageSource::Base64 { media_type, data } => {
                                        format!("data:{};base64,{}", media_type, data)
                                    }
                                };
                                
                                json_parts.push(serde_json::json!({
                                    "type": "image_url",
                                    "image_url": {
                                        "url": url
                                        // "detail": "auto" // Default
                                    }
                                }));
                            },
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
                                tool_call_id = Some(id);
                                text_acc = content; // Tool result content is simple string usually
                            },
                            // Audio/Video skipped for now

                        }
                    }
                    
                    if tool_call_id.is_some() || (!tool_calls.is_empty()) {
                        // If tool related, content is usually null or the text string
                         if text_acc.is_empty() {
                             final_content = serde_json::Value::Null;
                         } else {
                             final_content = serde_json::Value::String(text_acc);
                         }
                    } else if json_parts.iter().any(|p| p["type"] == "image_url") {
                        // Multi-modal content
                         final_content = serde_json::Value::Array(json_parts);
                    } else {
                        // Simple text
                        final_content = serde_json::Value::String(text_acc);
                    }
                }
            }

            result.push(OpenAIMessage {
                role: role.to_string(),
                content: final_content,
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
        extra_params: Option<serde_json::Value>,
    ) -> Result<StreamingResponse> {
        // Check for response_format in extra_params
        let response_format = if let Some(params) = &extra_params {
            if let Some(format_val) = params.get("response_format") {
                 serde_json::from_value(format_val.clone()).ok()
            } else {
                None
            }
        } else {
            None
        };

        let request = ChatRequest {
            model: model.to_string(),
            messages: Self::convert_messages(system_prompt, messages),
            temperature,
            max_tokens,
            tools: Self::convert_tools(tools),
            response_format,
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
        id: Option<String>,
        name: Option<String>,
        arguments: String,
    }

    let sse_buffer = crate::utils::SseBuffer::new();
    let string_buffer = String::new();
    // Map of index -> ToolCallState for parallel tool calls
    let current_tools: std::collections::HashMap<usize, ToolCallState> = std::collections::HashMap::new();

    futures::stream::unfold(
        (stream, sse_buffer, string_buffer, current_tools),
        move |(mut stream, mut bytes_buffer, mut text_buffer, mut current_tools)| async move {
            loop {
                // Try to extract a complete SSE message from buffer
                if let Some(pos) = text_buffer.find("\n\n") {
                    let message = text_buffer[..pos].to_string();
                    text_buffer = text_buffer[pos + 2..].to_string();

                    // Parse the SSE message
                    if let Some(data) = message.strip_prefix("data: ") {
                        if data.trim() == "[DONE]" {
                            return Some((Ok(StreamingChoice::Done), (stream, bytes_buffer, text_buffer, current_tools)));
                        }

                        match serde_json::from_str::<StreamChunk>(data) {
                            Ok(chunk) => {
                                if let Some(choice) = chunk.choices.first() {
                                    // Check for content
                                    if let Some(content) = &choice.delta.content {
                                        if !content.is_empty() {
                                            return Some((
                                                Ok(StreamingChoice::Message(content.clone())),
                                                (stream, bytes_buffer, text_buffer, current_tools),
                                            ));
                                        }
                                    }

                                    // Check for tool calls
                                    if let Some(tool_calls) = &choice.delta.tool_calls {
                                        for tc in tool_calls {
                                            let index = tc.index.unwrap_or(0);
                                            let state = current_tools.entry(index).or_insert(ToolCallState {
                                                id: None,
                                                name: None,
                                                arguments: String::new(),
                                            });

                                            // Update ID
                                            if let Some(id) = &tc.id {
                                                state.id = Some(id.clone());
                                            }

                                            // Update Name
                                            if let Some(func) = &tc.function {
                                                if let Some(name) = &func.name {
                                                    state.name = Some(name.clone());
                                                }
                                                // Update Arguments
                                                if let Some(args) = &func.arguments {
                                                    state.arguments.push_str(args);
                                                }
                                            }
                                        }
                                    }

                                    // Check if tool calls are complete
                                    if choice.finish_reason.as_deref() == Some("tool_calls") {
                                        // We need to drain the tools and emit them.
                                        // Since we can only emit one StreamingChoice per iteration of unfold,
                                        // we'll emit a single ParallelToolCalls event containing all of them.
                                        
                                        let mut tools_map = std::collections::HashMap::new();
                                        
                                        // Drain all tools
                                        for (index, state) in current_tools.drain() {
                                            if let (Some(id), Some(name)) = (state.id, state.name) {
                                                 let args: serde_json::Value = serde_json::from_str(&state.arguments)
                                                    .unwrap_or(serde_json::Value::Null);
                                                 
                                                 tools_map.insert(index, aagt_core::message::ToolCall {
                                                    id,
                                                    name,
                                                    arguments: args, 
                                                 });
                                            }
                                        }

                                        if !tools_map.is_empty() {
                                            return Some((
                                                Ok(StreamingChoice::ParallelToolCalls(tools_map)),
                                                (stream, bytes_buffer, text_buffer, current_tools),
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
                    // Loop again to process next message in buffer *before* reading more from stream
                    continue;
                }

                // Need more data
                match stream.next().await {
                    Some(Ok(bytes)) => {
                        match bytes_buffer.push_and_get_text(&bytes) {
                            Ok(new_text) => {
                                text_buffer.push_str(&new_text);
                            }
                            Err(e) => {
                                return Some((
                                    Err(e),
                                    (stream, bytes_buffer, text_buffer, current_tools),
                                ));
                            }
                        }
                    }
                    Some(Err(e)) => {
                        return Some((
                            Err(Error::Http(e)),
                            (stream, bytes_buffer, text_buffer, current_tools),
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
/// OpenAI Models
pub const GPT_4O_MINI: &str = "gpt-4o-mini";
/// GPT-4 Turbo
pub const GPT_4_TURBO: &str = "gpt-4-turbo";
/// GPT-3.5 Turbo
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

// --- Embeddings Implementation ---

use aagt_core::rag::Embeddings;

#[derive(Debug, Serialize)]
struct EmbeddingRequest {
    input: String,
    model: String,
}

#[derive(Debug, Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

#[async_trait]
impl Embeddings for OpenAI {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let request = EmbeddingRequest {
            input: text.to_string(),
            model: "text-embedding-3-small".to_string(), // Default to small, cheap model
        };

        let response = self.client
            .post(format!("{}/embeddings", self.base_url))
            .headers(self.build_headers()?)
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(Error::ProviderApi(format!(
                "OpenAI Embeddings API error {}: {}",
                status, text
            )));
        }

        let body: EmbeddingResponse = response.json().await
            .map_err(|e| Error::ProviderApi(format!("Failed to parse embedding response: {}", e)))?;

        body.data.first()
            .map(|d| d.embedding.clone())
            .ok_or_else(|| Error::ProviderApi("No embedding returned".to_string()))
    }
}
