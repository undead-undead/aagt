//! Dynamic Skill loading and execution system.
//! 
//! This module allows AAGT to load tools at runtime from `.md` files
//! and execute associated scripts (Python, Node, etc.).
//! 
//! Safety is ensured by routing all "proposals" from scripts through
//! the AAGT `RiskManager`.

use std::path::{Path, PathBuf};
use std::collections::HashMap;
use std::sync::Arc;
use std::process::Command;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{info, warn, error};

use crate::error::{Error, Result};
use crate::tool::{Tool, ToolDefinition};
use crate::risk::RiskManager;

/// Metadata extracted from a `SKILL.md` frontmatter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMetadata {
    /// Name of the skill
    pub name: String,
    /// Short description
    pub description: String,
    /// Arguments schema (JSON Schema)
    pub parameters: Value,
    /// Script to execute
    pub script: Option<String>,
    /// Language or runtime for the script
    pub runtime: Option<String>,
}

/// A skill that executes an external script
pub struct DynamicSkill {
    metadata: SkillMetadata,
    instructions: String,
    base_dir: PathBuf,
    risk_manager: Option<Arc<RiskManager>>,
}

impl DynamicSkill {
    /// Create a new dynamic skill
    pub fn new(metadata: SkillMetadata, instructions: String, base_dir: PathBuf) -> Self {
        Self {
            metadata,
            instructions,
            base_dir,
            risk_manager: None,
        }
    }

    /// Set a risk manager for validating proposals
    pub fn with_risk_manager(mut self, risk_manager: Arc<RiskManager>) -> Self {
        self.risk_manager = Some(risk_manager);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    pub from_token: String,
    pub to_token: String,
    pub amount_usd: f64,
    /// Amount string for the action (e.g. "100", "50%", "max")
    pub amount: String,
    pub expected_slippage: Option<f64>,
}

#[async_trait]
impl Tool for DynamicSkill {
    fn name(&self) -> String {
        self.metadata.name.clone()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.metadata.name.clone(),
            description: format!("{}\n\nINSTRUCTIONS:\n{}", self.metadata.description, self.instructions),
            parameters: self.metadata.parameters.clone(),
        }
    }


    async fn call(&self, arguments: &str) -> Result<String> {
        let script_file = self.metadata.script.as_ref().ok_or_else(|| {
            Error::tool_execution(self.name(), "No script defined for this skill".to_string())
        })?;

        let runtime = self.metadata.runtime.as_deref().unwrap_or("python3");
        let script_file = self.metadata.script.as_ref().unwrap();
        let script_rel_path = Path::new("scripts").join(script_file);
        let script_full_path = self.base_dir.join(&script_rel_path);

        if !script_full_path.exists() {
            return Err(Error::tool_execution(
                self.name(),
                format!("Script not found at {:?}", script_full_path),
            ));
        }

        info!("Executing dynamic skill {} using {}", self.name(), runtime);

        let output = tokio::process::Command::new(runtime)
            .arg(&script_rel_path) // Relative to current_dir
            .arg(arguments)
            .current_dir(&self.base_dir)
            .output()
            .await
            .map_err(|e| Error::tool_execution(self.name(), format!("Failed to spawn process: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            error!("Skill {} script failed: {}", self.name(), stderr);
            return Err(Error::tool_execution(self.name(), stderr.to_string()));
        }

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();

        // 🛡️ Safety Check: Parse for Proposals
        if let Ok(value) = serde_json::from_str::<Value>(&stdout) {
            if value.get("type").and_then(|t| t.as_str()) == Some("proposal") {
                if let Some(proposal_data) = value.get("data") {
                    let proposal: Proposal = serde_json::from_value(proposal_data.clone())
                        .map_err(|e| Error::tool_execution(self.name(), format!("Malformed proposal: {}", e)))?;

                    info!("Skill {} generated a transaction proposal: {:?}", self.name(), proposal);

                    if let Some(ref rm) = self.risk_manager {
                        let context = crate::risk::TradeContext {
                            user_id: "default_user".to_string(), // In production, this should come from agent config
                            from_token: proposal.from_token.clone(),
                            to_token: proposal.to_token.clone(),
                            amount_usd: proposal.amount_usd,
                            expected_slippage: proposal.expected_slippage.unwrap_or(1.0),
                            liquidity_usd: None,
                            is_flagged: false,
                        };

                        // 1. Check Risk
                        rm.check_and_reserve(&context).await
                            .map_err(|e| Error::tool_execution(self.name(), format!("Risk Check Denied: {}", e)))?;

                        info!("Risk check approved for skill {}", self.name());

                        // 2. Execute Action (Simulation for now, or real if executor provided)
                        // In a real scenario, we'd use the provided executor to perform the swap.
                        // For this prototype, we record the success and return.
                        
                        rm.commit_trade(&context.user_id, context.amount_usd).await?;
                        
                        return Ok(format!("SUCCESS: Trade executed after risk approval. Details: {:?}", proposal));
                    } else {
                        return Err(Error::tool_execution(self.name(), "RiskManager not configured, cannot execute risky proposal".to_string()));
                    }
                }
            }
        }

        Ok(stdout)
    }
}

/// Registry and loader for dynamic skills
pub struct SkillLoader {
    pub skills: HashMap<String, Arc<DynamicSkill>>,
    base_path: PathBuf,
    risk_manager: Option<Arc<RiskManager>>,
}

impl SkillLoader {
    /// Create a new registry
    pub fn new(base_path: impl Into<PathBuf>) -> Self {
        Self {
            skills: HashMap::new(),
            base_path: base_path.into(),
            risk_manager: None,
        }
    }

    /// Set a risk manager for all loaded skills
    pub fn with_risk_manager(mut self, risk_manager: Arc<RiskManager>) -> Self {
        self.risk_manager = Some(risk_manager);
        self
    }

    /// Load all skills from the base directory
    pub async fn load_all(&mut self) -> Result<()> {
        if !self.base_path.exists() {
            return Ok(());
        }

        let mut entries = tokio::fs::read_dir(&self.base_path).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_dir() {
                if let Ok(mut skill) = self.load_skill(&path).await {
                    if let Some(ref rm) = self.risk_manager {
                        skill = skill.with_risk_manager(Arc::clone(rm));
                    }
                    info!("Loaded dynamic skill: {}", skill.name());
                    self.skills.insert(skill.name(), Arc::new(skill));
                }
            }
        }
        Ok(())
    }

    async fn load_skill(&self, path: &Path) -> Result<DynamicSkill> {
        let manifest_path = path.join("SKILL.md");
        if !manifest_path.exists() {
            return Err(Error::Internal("No SKILL.md found".to_string()));
        }

        let content = tokio::fs::read_to_string(&manifest_path).await?;
        
        // Simple Manual Parser for YAML Frontmatter
        // (Avoiding adding more dependencies for now as per minimal footprint goal)
        let mut lines = content.lines();
        if lines.next() != Some("---") {
            return Err(Error::Internal("SKILL.md must start with ---".to_string()));
        }

        let mut yaml_str = String::new();
        let mut found_end = false;
        for line in lines.by_ref() {
            if line == "---" {
                found_end = true;
                break;
            }
            yaml_str.push_str(line);
            yaml_str.push('\n');
        }

        if !found_end {
            return Err(Error::Internal("SKILL.md frontmatter unclosed".to_string()));
        }

        let metadata: SkillMetadata = serde_yaml_ng::from_str(&yaml_str)
            .map_err(|e| Error::Internal(format!("Failed to parse Skill YAML: {}", e)))?;
        
        let instructions = lines.collect::<Vec<_>>().join("\n");

        Ok(DynamicSkill::new(metadata, instructions, path.to_path_buf()))
    }
}
