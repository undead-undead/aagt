use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use crate::error::{Error, Result};
use crate::tool::{Tool, ToolDefinition};
use aagt_qmd::HybridSearchEngine;

/// Tool for searching historical conversations and knowledge
pub struct SearchHistoryTool {
    engine: Arc<HybridSearchEngine>,
}

impl SearchHistoryTool {
    pub fn new(engine: Arc<HybridSearchEngine>) -> Self {
        Self { engine }
    }
}

#[async_trait]
impl Tool for SearchHistoryTool {
    fn name(&self) -> String {
        "search_history".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Search through past conversations, trading strategies, and knowledge using natural language or keywords. \
                Use this when you need context about a topic discussed previously or to find specific historical data.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query (natural language or keywords)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max number of results to return (default: 5)",
                        "default": 5
                    }
                },
                "required": ["query"]
            }),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            query: String,
            #[serde(default = "default_limit")]
            limit: usize,
        }
        fn default_limit() -> usize { 5 }

        let args: Args = serde_json::from_str(arguments)
            .map_err(|e| Error::ToolArguments {
                tool_name: self.name(),
                message: e.to_string(),
            })?;

        let results = self.engine.search(&args.query, args.limit)
            .map_err(|e| Error::Internal(format!("Search failed: {}", e)))?;

        if results.is_empty() {
            return Ok("No relevant history found.".to_string());
        }

        let mut output = format!("Found {} relevant matches:\n\n", results.len());
        for (i, res) in results.iter().enumerate() {
            let content = res.document.body.as_deref().unwrap_or("[No content]");
            output.push_str(&format!("[{}] {} (Score: {:.2})\n{}\n---\n", 
                i + 1, res.document.title, res.rrf_score, content));
        }

        Ok(output)
    }
}

/// Tool for saving important insights to long-term memory
pub struct RememberThisTool {
    engine: Arc<HybridSearchEngine>,
}

impl RememberThisTool {
    pub fn new(engine: Arc<HybridSearchEngine>) -> Self {
        Self { engine }
    }
}

#[async_trait]
impl Tool for RememberThisTool {
    fn name(&self) -> String {
        "remember_this".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Save a key insight, fact, or trading rule to your long-term memory. \
                Use this to ensure critical information is preserved and available for future retrieval.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "title": {
                        "type": "string",
                        "description": "Short mnemonic title for this memory"
                    },
                    "content": {
                        "type": "string",
                        "description": "The detail information to be remembered"
                    },
                    "collection": {
                        "type": "string",
                        "description": "Category (e.g., 'rules', 'preferences', 'insights')",
                        "default": "general"
                    }
                },
                "required": ["title", "content"]
            }),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            title: String,
            content: String,
            #[serde(default = "default_coll")]
            collection: String,
        }
        fn default_coll() -> String { "general".to_string() }

        let args: Args = serde_json::from_str(arguments)
            .map_err(|e| Error::ToolArguments {
                tool_name: self.name(),
                message: e.to_string(),
            })?;

        let timestamp = chrono::Utc::now().timestamp_millis();
        let path = format!("memory_{}", timestamp);

        self.engine.index_document(&args.collection, &path, &args.title, &args.content)
            .map_err(|e| Error::Internal(format!("Storage failed: {}", e)))?;

        Ok(format!("Memory successfully saved as '{}' in collection '{}'.", args.title, args.collection))
    }
}
