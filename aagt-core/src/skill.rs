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

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{info, warn, error};

use crate::error::{Error, Result};
use crate::tool::{Tool, ToolDefinition};
use crate::risk::RiskManager;
use crate::strategy::{Action, ActionExecutor};

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

/// Configuration for skill execution
#[derive(Debug, Clone)]
pub struct SkillExecutionConfig {
    /// Maximum execution time in seconds
    pub timeout_secs: u64,
    /// Maximum output size in bytes (to prevent memory exhaustion)
    pub max_output_bytes: usize,
    /// Whether to allow network access (future: implement via sandbox)
    pub allow_network: bool,
    /// Custom environment variables
    pub env_vars: HashMap<String, String>,
}

impl Default for SkillExecutionConfig {
    fn default() -> Self {
        Self {
            timeout_secs: 30,
            max_output_bytes: 1024 * 1024, // 1MB
            allow_network: false,
            env_vars: HashMap::new(),
        }
    }
}

/// A skill that executes an external script
pub struct DynamicSkill {
    metadata: SkillMetadata,
    instructions: String,
    base_dir: PathBuf,
    risk_manager: Option<Arc<RiskManager>>,
    executor: Option<Arc<dyn ActionExecutor>>,
    execution_config: SkillExecutionConfig,
}

impl DynamicSkill {
    /// Create a new dynamic skill
    pub fn new(metadata: SkillMetadata, instructions: String, base_dir: PathBuf) -> Self {
        Self {
            metadata,
            instructions,
            base_dir,
            risk_manager: None,
            executor: None,
            execution_config: SkillExecutionConfig::default(),
        }
    }

    /// Set a risk manager for validating proposals
    pub fn with_risk_manager(mut self, risk_manager: Arc<RiskManager>) -> Self {
        self.risk_manager = Some(risk_manager);
        self
    }

    /// Set an action executor for executing approved proposals
    pub fn with_executor(mut self, executor: Arc<dyn ActionExecutor>) -> Self {
        self.executor = Some(executor);
        self
    }

    /// Set custom execution configuration
    pub fn with_execution_config(mut self, config: SkillExecutionConfig) -> Self {
        self.execution_config = config;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    pub from_token: String,
    pub to_token: String,
    pub amount_usd: rust_decimal::Decimal,
    /// Amount string for the action (e.g. "100", "50%", "max")
    pub amount: String,
    pub expected_slippage: Option<rust_decimal::Decimal>,
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

        let script_rel_path = Path::new("scripts").join(script_file);
        let script_full_path = self.base_dir.join(&script_rel_path);

        if !script_full_path.exists() {
            return Err(Error::tool_execution(
                self.name(),
                format!("Script not found at {:?}", script_full_path),
            ));
        }

        let runtime = self.metadata.runtime.as_deref().unwrap_or("python3");
        info!("Executing dynamic skill {} using {}", self.name(), runtime);

        // Use configured timeout
        let timeout_duration = std::time::Duration::from_secs(self.execution_config.timeout_secs);
        
        let mut cmd = tokio::process::Command::new(runtime);
        cmd.arg(&script_rel_path)
            .arg(arguments)
            .current_dir(&self.base_dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        // Apply environment variables from config
        for (key, value) in &self.execution_config.env_vars {
            cmd.env(key, value);
        }

        // Restrict network access if configured (Linux-only via basic example)
        if !self.execution_config.allow_network {
            // Note: True sandboxing requires container/VM. This is a placeholder.
            // In production, consider using firejail, nsjail, or containers.
            cmd.env("NO_PROXY", "*");
        }

        let child = cmd.spawn()
            .map_err(|e| Error::tool_execution(self.name(), format!("Failed to spawn process: {}", e)))?;

        // Save process ID before moving child
        let pid = child.id();
        
        let output_future = child.wait_with_output();
        let output = match tokio::time::timeout(timeout_duration, output_future).await {
            Ok(Ok(output)) => output,
            Ok(Err(e)) => {
                return Err(Error::tool_execution(self.name(), format!("Process failed: {}", e)));
            }
            Err(_) => {
                // Timeout: forcefully kill the process using saved PID
                warn!("Skill {} timed out after {}s, killing process {}", self.name(), timeout_duration.as_secs(), pid.unwrap_or(0));
                if let Some(pid) = pid {
                    // Use system kill command
                    #[cfg(unix)]
                    {
                        use std::process::Command;
                        let _ = Command::new("kill").arg("-9").arg(pid.to_string()).status();
                    }
                }
                return Err(Error::tool_execution(
                    self.name(), 
                    format!("Skill timed out after {}s", timeout_duration.as_secs())
                ));
            }
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            error!("Skill {} script failed: {}", self.name(), stderr);
            return Err(Error::tool_execution(self.name(), stderr.to_string()));
        }

        // Check output size limit
        if output.stdout.len() > self.execution_config.max_output_bytes {
            return Err(Error::tool_execution(
                self.name(),
                format!("Script output exceeds size limit of {} bytes", self.execution_config.max_output_bytes)
            ));
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
                            expected_slippage: proposal.expected_slippage.unwrap_or(rust_decimal_macros::dec!(1.0)),
                            liquidity_usd: None,
                            is_flagged: false,
                        };

                        // 1. Check Risk
                        rm.check_and_reserve(&context).await
                            .map_err(|e| Error::tool_execution(self.name(), format!("Risk Check Denied: {}", e)))?;

                        info!("Risk check approved for skill {}", self.name());

                        // 2. Execute Action
                        if let Some(ref executor) = self.executor {
                             // Map Proposal to Action::Swap
                             let action = Action::Swap {
                                 from_token: proposal.from_token,
                                 to_token: proposal.to_token,
                                 amount: proposal.amount,
                             };
                             
                             let pipeline_ctx = crate::pipeline::Context::new(format!("Skill execution: {}", self.name()));
                             // We could pass more context if needed
                             
                             let result = executor.execute(&action, &pipeline_ctx).await
                                .map_err(|e| Error::tool_execution(self.name(), format!("Execution Failed: {}", e)))?;
                                
                             // Once executed success, we confirm the trade to RiskManager (commit)
                             rm.commit_trade(&context.user_id, context.amount_usd).await?;
                             
                             return Ok(format!("SUCCESS: Trade executed: {}", result));
                        } else {
                            // Simulation Mode (Legacy behavior)
                            // Still commit the risk usage as "Paper Trading"
                            rm.commit_trade(&context.user_id, context.amount_usd).await?;
                            return Ok(format!("SIMULATION SUCCESS: Trade approved by risk manager but NO EXECUTOR configured. Proposal: {:?}", proposal));
                        }
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
    executor: Option<Arc<dyn ActionExecutor>>,
}

impl SkillLoader {
    /// Create a new registry
    pub fn new(base_path: impl Into<PathBuf>) -> Self {
        Self {
            skills: HashMap::new(),
            base_path: base_path.into(),
            risk_manager: None,
            executor: None,
        }
    }

    /// Set a risk manager for all loaded skills
    pub fn with_risk_manager(mut self, risk_manager: Arc<RiskManager>) -> Self {
        self.risk_manager = Some(risk_manager);
        self
    }

    /// Set an executor for all loaded skills
    pub fn with_executor(mut self, executor: Arc<dyn ActionExecutor>) -> Self {
        self.executor = Some(executor);
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
                    if let Some(ref exec) = self.executor {
                        skill = skill.with_executor(Arc::clone(exec));
                    }
                    info!("Loaded dynamic skill: {}", skill.name());
                    self.skills.insert(skill.name(), Arc::new(skill));
                }
            }
        }
        Ok(())
    }

    pub async fn load_skill(&self, path: &Path) -> Result<DynamicSkill> {
        let manifest_path = path.join("SKILL.md");
        if !manifest_path.exists() {
            return Err(Error::Internal("No SKILL.md found".to_string()));
        }

        let content = tokio::fs::read_to_string(&manifest_path).await?;
        
        // Find frontmatter delimiters
        let start_delimiter = "---\n";
        let end_delimiter = "\n---";
        
        // Normalize line endings for delimiter search? 
        // Or just search based on robust logic.
        // Let's assume \n or \r\n. 
        // We will look for "\n---" which indicates the end delimiter on its own line.
        
        let yaml_str;
        let instructions;

        // Ensure file starts with YAML frontmatter
        if content.starts_with(start_delimiter) || content.starts_with("---\r\n") {
             // Find end of frontmatter
             if let Some(end_idx) = content[4..].find(end_delimiter) {
                 let actual_end_idx = end_idx + 4; // Add back the initial offset
                 yaml_str = &content[4..actual_end_idx]; // Exclude delimiters
                 
                 // Instructions start after the end delimiter + delimiter length (4 chars for \n---)
                 // content[actual_end_idx] starts with \n. 
                 // \n--- is 4 chars.
                 let rest_start = actual_end_idx + 4;
                 if rest_start < content.len() {
                     instructions = content[rest_start..].trim().to_string();
                 } else {
                     instructions = String::new();
                 }
             } else {
                 return Err(Error::Internal("SKILL.md frontmatter unclosed (missing closing ---)".to_string()));
             }
        } else {
             return Err(Error::Internal("SKILL.md must start with ---".to_string()));
        }

        let metadata: SkillMetadata = serde_yaml_ng::from_str(yaml_str)
            .map_err(|e| Error::Internal(format!("Failed to parse Skill YAML: {}", e)))?;
        
        Ok(DynamicSkill::new(metadata, instructions, path.to_path_buf()))
    }
}
