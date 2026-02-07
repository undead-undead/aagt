//! Tool system for AI agents
//!
//! Provides the core abstraction for defining tools that AI agents can call.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::error::Error;

pub mod code_interpreter;
pub mod cron;
pub mod delegation;
pub mod memory;

pub use cron::CronTool;
pub use delegation::DelegateTool;
pub use memory::{RememberThisTool, SearchHistoryTool, TieredSearchTool, FetchDocumentTool};

/// Definition of a tool that can be sent to the LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Name of the tool
    pub name: String,
    /// Description for the LLM
    pub description: String,
    /// JSON Schema for parameters (Legacy/API)
    pub parameters: serde_json::Value,
    /// TypeScript interface definition (Preferred for System Prompt)
    pub parameters_ts: Option<String>,
    /// Whether this is a binary tool (e.g. Wasm)
    #[serde(default)]
    pub is_binary: bool,
    /// Whether the tool is verified/trusted
    #[serde(default)]
    pub is_verified: bool,
}

/// Trait for implementing tools that AI agents can call
#[async_trait]
pub trait Tool: Send + Sync {
    /// The name of this tool
    /// The name of this tool
    fn name(&self) -> String;

    /// Get the tool definition for the LLM
    async fn definition(&self) -> ToolDefinition;

    /// Execute the tool with the given arguments (JSON string)
    async fn call(&self, arguments: &str) -> anyhow::Result<String>;
}

#[derive(Clone)]
pub struct ToolSet {
    tools: HashMap<String, Arc<dyn Tool>>,
    /// Cached definitions to avoid async calls during prompt generation
    cached_definitions: Arc<parking_lot::RwLock<HashMap<String, ToolDefinition>>>,
}

impl Default for ToolSet {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolSet {
    /// Create an empty toolset
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            cached_definitions: Arc::new(parking_lot::RwLock::new(HashMap::new())),
        }
    }

    /// Add a tool to the set
    pub fn add<T: Tool + 'static>(&mut self, tool: T) -> &mut Self {
        self.tools.insert(tool.name().to_string(), Arc::new(tool));
        self
    }

    /// Add a shared tool to the set
    pub fn add_shared(&mut self, tool: Arc<dyn Tool>) -> &mut Self {
        self.tools.insert(tool.name().to_string(), tool);
        self
    }

    /// Get a tool by name
    pub fn get(&self, name: &str) -> Option<&Arc<dyn Tool>> {
        self.tools.get(name)
    }

    /// Check if a tool exists
    pub fn contains(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    /// Get all tool definitions
    pub async fn definitions(&self) -> Vec<ToolDefinition> {
        let mut defs = Vec::new();
        for (name, tool) in &self.tools {
            // Check cache in a small block to ensure guard is dropped
            let cached = {
                self.cached_definitions.read().get(name).cloned()
            };

            if let Some(def) = cached {
                defs.push(def);
            } else {
                let def = tool.definition().await;
                self.cached_definitions.write().insert(name.clone(), def.clone());
                defs.push(def);
            }
        }
        defs
    }

    /// Call a tool by name
    pub async fn call(&self, name: &str, arguments: &str) -> anyhow::Result<String> {
        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| Error::ToolNotFound(name.to_string()))?;

        tool.call(arguments).await
    }

    /// Get the number of tools
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// Iterate over tools
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Arc<dyn Tool>)> {
        self.tools.iter()
    }
}

#[async_trait::async_trait]
impl crate::agent::context::ContextInjector for ToolSet {
    async fn inject(&self) -> crate::error::Result<Vec<crate::agent::message::Message>> {
        if self.tools.is_empty() {
            return Ok(Vec::new());
        }

        let mut content = String::from("## Tool Definitions (TypeScript)\n\n");
        content.push_str("You have access to the following tools. Use them to fulfill the user's request.\n\n");

        // Sort for determinism
        let mut sorted_tools: Vec<_> = self.tools.iter().collect();
        sorted_tools.sort_by_key(|(k, _)| *k);

        for (name, tool) in sorted_tools {
            let cached_def = {
                self.cached_definitions.read().get(name).cloned()
            };

            let def = if let Some(d) = cached_def {
                d
            } else {
                let d = tool.definition().await;
                self.cached_definitions.write().insert(name.clone(), d.clone());
                d
            };
            
            content.push_str(&format!("### {}\n{}\n", name, def.description));
            if let Some(ts) = def.parameters_ts {
                content.push_str("```typescript\n");
                content.push_str(&ts);
                if !ts.ends_with('\n') {
                    content.push('\n');
                }
                content.push_str("```\n\n");
            } else {
                // Fallback to JSON if TS missing
                content.push_str("```json\n");
                content.push_str(&serde_json::to_string_pretty(&def.parameters).unwrap_or_default());
                content.push_str("\n```\n\n");
            }
        }

        Ok(vec![crate::agent::message::Message::system(content)])
    }
}

/// Builder for creating a ToolSet
pub struct ToolSetBuilder {
    tools: Vec<Arc<dyn Tool>>,
}

impl Default for ToolSetBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolSetBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self { tools: Vec::new() }
    }

    /// Add a tool
    pub fn tool<T: Tool + 'static>(mut self, tool: T) -> Self {
        self.tools.push(Arc::new(tool));
        self
    }

    /// Add a shared tool
    pub fn shared_tool(mut self, tool: Arc<dyn Tool>) -> Self {
        self.tools.push(tool);
        self
    }

    /// Build the ToolSet
    pub fn build(self) -> ToolSet {
        let mut toolset = ToolSet::new();
        for tool in self.tools {
            toolset.add_shared(tool);
        }
        toolset
    }
}

/// Helper macro for creating simple tools
/// 
/// # Example
/// ```ignore
/// simple_tool!(
///     name: "get_time",
///     description: "Get the current time",
///     handler: |_args| async {
///         Ok(chrono::Utc::now().to_rfc3339())
///     }
/// );
/// ```
#[macro_export]
macro_rules! simple_tool {
    (
        name: $name:expr,
        description: $desc:expr,
        parameters: $params:expr,
        handler: $handler:expr
    ) => {{
        struct SimpleTool;

        #[async_trait::async_trait]
        impl $crate::tool::Tool for SimpleTool {
            fn name(&self) -> String {
                $name.to_string()
            }

            async fn definition(&self) -> $crate::tool::ToolDefinition {
                $crate::tool::ToolDefinition {
                    name: $name.to_string(),
                    description: $desc.to_string(),
                    parameters: $params,
                }
            }

            async fn call(&self, arguments: &str) -> anyhow::Result<String> {
                let handler = $handler;
                handler(arguments).await
            }
        }

        SimpleTool
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EchoTool;

    #[async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> String {
            "echo".to_string()
        }

        async fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "echo".to_string(),
                description: "Echo back the input".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "message": {
                            "type": "string",
                            "description": "Message to echo"
                        }
                    },
                    "required": ["message"]
                }),
                parameters_ts: None,
                is_binary: false,
                is_verified: true, // Internal tools are verified
            }
        }

        async fn call(&self, arguments: &str) -> anyhow::Result<String> {
            #[derive(Deserialize)]
            struct Args {
                message: String,
            }
            let args: Args = serde_json::from_str(arguments)
                .map_err(|e| Error::ToolArguments {
                    tool_name: "echo".to_string(),
                    message: e.to_string(),
                })?;
            Ok(args.message)
        }
    }

    #[tokio::test]
    async fn test_toolset() {
        let mut toolset = ToolSet::new();
        toolset.add(EchoTool);

        assert!(toolset.contains("echo"));
        assert_eq!(toolset.len(), 1);

        let result = toolset
            .call("echo", r#"{"message": "hello"}"#)
            .await
            .expect("call should succeed");
        assert_eq!(result, "hello");
    }
}
