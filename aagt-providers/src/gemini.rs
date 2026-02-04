//! Google Gemini provider implementation



use async_trait::async_trait;
use futures::{Stream, StreamExt};
use reqwest::header::CONTENT_TYPE;
use serde::{Deserialize, Serialize};

use crate::{Error, Result, Message, StreamingChoice, StreamingResponse, ToolDefinition, Provider, HttpConfig};
use aagt_core::message::{Role, Content};

const GEMINI_API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta/models";

/// Gemini API client
pub struct Gemini {
    client: reqwest::Client,
    api_key: String,
}

impl Gemini {
    /// Create from API key
    pub fn new(api_key: impl Into<String>) -> Result<Self> {
        let config = HttpConfig::default();
        let client = config.build_client()?;

        Ok(Self {
            client,
            api_key: api_key.into(),
        })
    }

    /// Create from environment variable
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("GEMINI_API_KEY")
            .map_err(|_| Error::ProviderAuth("GEMINI_API_KEY not set".to_string()))?;
        Self::new(api_key)
    }
}

/// Gemini request format
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    generation_config: Option<GenerationConfig>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<GeminiTool>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GeminiContent {
    role: String,
    parts: Vec<Part>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum Part {
    Text { text: String },
    FunctionCall { function_call: FunctionCall },
    FunctionResponse { function_response: FunctionResponse },
}

#[derive(Debug, Serialize, Deserialize)]
struct FunctionCall {
    name: String,
    args: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
struct FunctionResponse {
    name: String,
    response: serde_json::Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u64>,
}

#[derive(Debug, Serialize)]
struct GeminiTool {
    function_declarations: Vec<FunctionDeclaration>,
}

#[derive(Debug, Serialize)]
struct FunctionDeclaration {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

/// Streaming response chunk
#[derive(Debug, Deserialize)]
struct StreamChunk {
    candidates: Option<Vec<Candidate>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Candidate {
    content: Option<CandidateContent>,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CandidateContent {
    parts: Option<Vec<ResponsePart>>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ResponsePart {
    Text {
        text: String,
    },
    FunctionCall {
        #[serde(rename = "functionCall")]
        function_call: ResponseFunctionCall,
    },
    Thought {
        thought: String,
    },
}

#[derive(Debug, Deserialize)]
struct ResponseFunctionCall {
    name: String,
    args: serde_json::Value,
}

impl Gemini {
    fn convert_messages(messages: Vec<Message>) -> Vec<GeminiContent> {
        messages
            .into_iter()
            .filter(|m| m.role != Role::System)
            .map(|msg| {
                let role = match msg.role {
                    Role::User | Role::Tool => "user",
                    Role::Assistant => "model",
                    Role::System => "user",
                };

                let parts = match msg.content {
                    Content::Text(text) => vec![Part::Text { text }],
                    Content::Parts(content_parts) => content_parts
                        .into_iter()
                        .filter_map(|p| match p {
                            aagt_core::message::ContentPart::Text { text } => Some(Part::Text { text }),
                            aagt_core::message::ContentPart::ToolCall { name, arguments, .. } => {
                                Some(Part::FunctionCall {
                                    function_call: FunctionCall {
                                        name,
                                        args: arguments,
                                    }
                                })
                            },
                            aagt_core::message::ContentPart::ToolResult { name, content, .. } => {
                                // Gemini requires a name here. If it's missing, we are in trouble.
                                // We fallback to "unknown" or hope caller provided it.
                                let name = name.unwrap_or_else(|| "unknown".to_string());
                                
                                // Parse content as JSON if possible, otherwise wrap string
                                let response_json = match serde_json::from_str::<serde_json::Value>(&content) {
                                    Ok(v) => v,
                                    Err(_) => serde_json::json!({ "result": content })
                                };
                                
                                Some(Part::FunctionResponse {
                                    function_response: FunctionResponse {
                                        name,
                                        response: response_json,
                                    }
                                })
                            },
                            _ => None // Images not supported yet
                        })
                        .collect(),
                };

                GeminiContent {
                    role: role.to_string(),
                    parts,
                }
            })
            .collect()
    }

    fn convert_tools(tools: Vec<ToolDefinition>) -> Vec<GeminiTool> {
        if tools.is_empty() {
            return vec![];
        }

        vec![GeminiTool {
            function_declarations: tools
                .into_iter()
                .map(|t| FunctionDeclaration {
                    name: t.name,
                    description: t.description,
                    parameters: t.parameters,
                })
                .collect(),
        }]
    }
}

#[async_trait]
impl Provider for Gemini {
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
        let request = GeminiRequest {
            contents: Self::convert_messages(messages),
            system_instruction: system_prompt.map(|s| GeminiContent {
                role: "user".to_string(),
                parts: vec![Part::Text { text: s.to_string() }],
            }),
            generation_config: Some(GenerationConfig {
                temperature,
                max_output_tokens: max_tokens,
            }),
            tools: Self::convert_tools(tools),
        };

        let url = format!(
            "{}{}:streamGenerateContent?alt=sse&key={}",
            GEMINI_API_BASE, model, self.api_key
        );

        let response = self
            .client
            .post(&url)
            .header(CONTENT_TYPE, "application/json")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(Error::ProviderApi(format!(
                "Gemini API error {}: {}",
                status, text
            )));
        }

        let stream = response.bytes_stream();
        let parsed_stream = parse_gemini_stream(stream);

        Ok(StreamingResponse::from_stream(parsed_stream))
    }

    fn name(&self) -> &'static str {
        "gemini"
    }
}

/// Parse SSE stream from Gemini
fn parse_gemini_stream<S>(
    stream: S,
) -> impl Stream<Item = std::result::Result<StreamingChoice, Error>>
where
    S: Stream<Item = std::result::Result<bytes::Bytes, reqwest::Error>> + Send + Unpin + 'static,
{
    let sse_buffer = crate::utils::SseBuffer::new();
    let string_buffer = String::new();
    let tool_call_counter: usize = 0;

    futures::stream::unfold(
        (stream, sse_buffer, string_buffer, tool_call_counter),
        move |(mut stream, mut bytes_buffer, mut text_buffer, mut tool_counter)| async move {
            loop {
                // Try to extract complete SSE message
                if let Some(pos) = text_buffer.find("\n\n") {
                    let line = text_buffer[..pos].to_string();
                    text_buffer = text_buffer[pos + 2..].to_string();

                    if let Some(data) = line.strip_prefix("data: ") {
                        match serde_json::from_str::<StreamChunk>(data) {
                            Ok(chunk) => {
                                if let Some(candidates) = chunk.candidates {
                                    if let Some(candidate) = candidates.first() {
                                        // Check finish reason
                                        if candidate.finish_reason.as_deref() == Some("STOP") {
                                            return Some((
                                                Ok(StreamingChoice::Done),
                                                (stream, bytes_buffer, text_buffer, tool_counter),
                                            ));
                                        }

                                        if let Some(content) = &candidate.content {
                                            if let Some(parts) = &content.parts {
                                                for part in parts {
                                                    match part {
                                                        ResponsePart::Text { text } => {
                                                            if !text.is_empty() {
                                                                 return Some((
                                                                    Ok(StreamingChoice::Message(text.clone())),
                                                                    (stream, bytes_buffer, text_buffer, tool_counter),
                                                                ));
                                                            }
                                                        }
                                                        ResponsePart::FunctionCall { function_call } => {
                                                            tool_counter += 1;
                                                            return Some((
                                                                Ok(StreamingChoice::ToolCall {
                                                                    id: format!("call_{}", tool_counter),
                                                                    name: function_call.name.clone(),
                                                                    arguments: function_call.args.clone(),
                                                                }),
                                                                (stream, bytes_buffer, text_buffer, tool_counter),
                                                            ));
                                                        }
                                                        ResponsePart::Thought { thought } => {
                                                            if !thought.is_empty() {
                                                                return Some((
                                                                    Ok(StreamingChoice::Thought(thought.clone())),
                                                                    (stream, bytes_buffer, text_buffer, tool_counter),
                                                                ));
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::debug!("Failed to parse Gemini chunk: {}", e);
                            }
                        }
                    }
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
                                    (stream, bytes_buffer, text_buffer, tool_counter),
                                ));
                            }
                        }
                    }
                    Some(Err(e)) => {
                        return Some((
                            Err(Error::Http(e)),
                            (stream, bytes_buffer, text_buffer, tool_counter),
                        ));
                    }
                    None => return None,
                }
            }
        },
    )
}

/// Common model constants
/// Gemini 2.0 Flash
pub const GEMINI_2_0_FLASH: &str = "gemini-2.0-flash-exp";
/// Gemini 2.0 Flash Thinking
pub const GEMINI_2_0_FLASH_THINKING: &str = "gemini-2.0-flash-thinking-exp";
/// Gemini 1.5 Pro
pub const GEMINI_1_5_PRO: &str = "gemini-1.5-pro";
/// Gemini 1.5 Flash
pub const GEMINI_1_5_FLASH: &str = "gemini-1.5-flash";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_conversion() {
        let messages = vec![
            Message::user("Hello"),
            Message::assistant("Hi!"),
        ];

        let converted = Gemini::convert_messages(messages);
        assert_eq!(converted.len(), 2);
        assert_eq!(converted[0].role, "user");
        assert_eq!(converted[1].role, "model");
    }

    #[test]
    fn test_tool_conversion() {
        let tools = vec![ToolDefinition {
            name: "test".to_string(),
            description: "A test tool".to_string(),
            parameters: serde_json::json!({"type": "object"}),
        }];

        let converted = Gemini::convert_tools(tools);
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0].function_declarations.len(), 1);
    }
}
