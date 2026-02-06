use aagt_core::prelude::*;
use aagt_core::agent::core::{ToolPolicy, RiskyToolPolicy};
use aagt_core::skills::tool::{Tool, ToolDefinition};
use aagt_core::error::Result;
use async_trait::async_trait;
use serde_json::json;
use std::collections::HashMap;

struct DangerousTool;

#[async_trait]
impl Tool for DangerousTool {
    fn name(&self) -> String { "nuke_db".to_string() }
    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "nuke_db".to_string(),
            description: "Delete everything".to_string(),
            parameters: json!({"type": "object"}),
        }
    }
    async fn call(&self, _args: &str) -> anyhow::Result<String> {
        Ok("safe".to_string())
    }
}

struct SafeTool;

#[async_trait]
impl Tool for SafeTool {
    fn name(&self) -> String { "read_db".to_string() }
    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "read_db".to_string(),
            description: "Read data".to_string(),
            parameters: json!({"type": "object"}),
        }
    }
    async fn call(&self, _args: &str) -> anyhow::Result<String> {
        Ok("dangerous".to_string())
    }
}

// Mock provider needed for Agent construction
use aagt_core::agent::provider::Provider;
use aagt_core::agent::streaming::StreamingResponse;
struct MockProvider;
#[async_trait]
impl Provider for MockProvider {
    async fn stream_completion(&self, _: &str, _: Option<&str>, _: Vec<Message>, _: Vec<ToolDefinition>, _: Option<f64>, _: Option<u64>, _: Option<serde_json::Value>) -> Result<StreamingResponse> {
        unimplemented!()
    }
    fn name(&self) -> &'static str { "mock" }
}

#[tokio::test]
async fn test_tool_policy_disabled() {
    let mut overrides = HashMap::new();
    overrides.insert("nuke_db".to_string(), ToolPolicy::Disabled);
    
    let policy = RiskyToolPolicy {
        default_policy: ToolPolicy::Auto,
        overrides,
    };

    let agent = Agent::builder(MockProvider)
        .tool(DangerousTool)
        .tool_policy(policy)
        .build()
        .unwrap();

    let result = agent.call_tool("nuke_db", "{}").await;
    match result {
        Err(aagt_core::error::Error::ToolExecution{message, ..}) => {
            assert!(message.contains("disabled by policy"));
        },
        _ => panic!("Should have failed with policy error"),
    }
}

#[tokio::test]
async fn test_tool_policy_approval() {
    let mut overrides = HashMap::new();
    overrides.insert("nuke_db".to_string(), ToolPolicy::RequiresApproval);
    
    let policy = RiskyToolPolicy {
        default_policy: ToolPolicy::Auto,
        overrides,
    };

    let agent = Agent::builder(MockProvider)
        .tool(DangerousTool)
        .tool_policy(policy)
        .build()
        .unwrap();

    let result = agent.call_tool("nuke_db", "{}").await;
    match result {
        Err(aagt_core::error::Error::ToolApprovalRequired { tool_name }) => {
            assert_eq!(tool_name, "nuke_db");
        },
        _ => panic!("Should have failed with approval required"),
    }
}

#[tokio::test]
async fn test_tool_policy_auto() {
    let policy = RiskyToolPolicy::default(); // Auto by default

    let agent = Agent::builder(MockProvider)
        .tool(SafeTool)
        .tool_policy(policy)
        .build()
        .unwrap();

    let result = agent.call_tool("read_db", "{}").await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "Data read");
}
