//! Tool system for AI agents
//!
//! Provides the core abstraction for defining tools that AI agents can call.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::error::{Error, Result};

pub mod code_interpreter;
pub mod memory;

pub use memory::{RememberThisTool, SearchHistoryTool};

/// Definition of a tool that can be sent to the LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Name of the tool
    pub name: String,
    /// Description for the LLM
    pub description: String,
    /// JSON Schema for parameters
    pub parameters: serde_json::Value,
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

/// A collection of tools available to an agent
pub struct ToolSet {
    tools: HashMap<String, Arc<dyn Tool>>,
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
        for tool in self.tools.values() {
            defs.push(tool.definition().await);
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
