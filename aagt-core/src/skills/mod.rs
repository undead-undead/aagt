pub mod tool;
pub mod capabilities;
pub mod runtime;
pub mod compiler;

use std::path::{Path, PathBuf};
use std::collections::HashMap;
use std::sync::Arc;
use dashmap::DashMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{info, warn};

use crate::error::{Error, Result};
use crate::skills::tool::{Tool, ToolDefinition};
use crate::agent::context::ContextInjector;
use crate::agent::message::Message;
#[cfg(feature = "trading")]
use crate::trading::risk::RiskManager;
#[cfg(feature = "trading")]
use crate::trading::strategy::{Action, ActionExecutor};

/// Metadata extracted from a `SKILL.md` frontmatter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMetadata {
    /// Name of the skill
    pub name: String,
    /// Short description
    pub description: String,
    /// Optional homepage URL
    pub homepage: Option<String>,
    /// Arguments schema (JSON Schema) - DEPRECATED: use parameters_ts
    pub parameters: Option<Value>,
    /// Arguments as TypeScript interface (Preferred)
    pub interface: Option<String>,
    /// Script to execute
    pub script: Option<String>,
    /// Language or runtime for the script
    pub runtime: Option<String>,
    /// Standard ClawHub metadata object
    #[serde(default)]
    pub metadata: Value,
    /// Kind of skill (e.g., 'tool', 'knowledge', 'agent')
    #[serde(default = "default_skill_kind")]
    pub kind: String,
    /// Optional usage guidelines for LLM reasoning
    pub usage_guidelines: Option<String>,
}

fn default_skill_kind() -> String {
    "tool".to_string()
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
    #[cfg(feature = "trading")]
    risk_manager: Option<Arc<RiskManager>>,
    #[cfg(feature = "trading")]
    executor: Option<Arc<dyn ActionExecutor>>,
    execution_config: SkillExecutionConfig,
    wasm_runtime: Arc<crate::skills::runtime::WasmRuntime>,
}

impl DynamicSkill {
    /// Create a new dynamic skill
    pub fn new(metadata: SkillMetadata, instructions: String, base_dir: PathBuf) -> Self {
        Self {
            metadata,
            instructions,
            base_dir,
            #[cfg(feature = "trading")]
            risk_manager: None,
            #[cfg(feature = "trading")]
            executor: None,
            execution_config: SkillExecutionConfig::default(),
            wasm_runtime: Arc::new(crate::skills::runtime::WasmRuntime::new().expect("Failed to init WasmRuntime")),
        }
    }

    /// Set a risk manager for validating proposals
    #[cfg(feature = "trading")]
    pub fn with_risk_manager(mut self, risk_manager: Arc<RiskManager>) -> Self {
        self.risk_manager = Some(risk_manager);
        self
    }

    /// Set an action executor for executing approved proposals
    #[cfg(feature = "trading")]
    pub fn with_executor(mut self, executor: Arc<dyn ActionExecutor>) -> Self {
        self.executor = Some(executor);
        self
    }

    /// Set custom execution configuration
    pub fn with_execution_config(mut self, config: SkillExecutionConfig) -> Self {
        self.execution_config = config;
        self
    }

    /// Access metadata
    pub fn metadata(&self) -> &SkillMetadata {
        &self.metadata
    }
}

#[cfg(feature = "trading")]
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
            description: self.metadata.description.clone(),
            parameters: self.metadata.parameters.clone().unwrap_or(json!({})),
            parameters_ts: self.metadata.interface.clone(),
            is_binary: self.metadata.runtime.as_deref() == Some("wasm"),
            is_verified: false, // Default to unverified
            usage_guidelines: self.metadata.usage_guidelines.clone(),
        }
    }



    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let runtime_type = self.metadata.runtime.as_deref().unwrap_or("python3");

        let interpreter = match runtime_type {
            "python" | "python3" => "python3",
            "bash" | "sh" => "bash",
            "node" | "js" => "node",
            "wasm" => "wasm",
            lang => lang
        };

        if interpreter == "wasm" {
            let wasm_file = self.metadata.script.as_ref().ok_or_else(|| {
                Error::tool_execution(self.name(), "No wasm file defined for this skill".to_string())
            })?;
            let wasm_path = self.base_dir.join("scripts").join(wasm_file);
            info!(tool = %self.name(), "Executing Wasm skill");
            return self.wasm_runtime.call(&wasm_path, arguments).map_err(|e| e.into());
        }

        let script_file = self.metadata.script.as_ref().ok_or_else(|| {
             Error::tool_execution(self.name(), "No script defined for this skill".to_string())
        })?;

        let script_rel_path = Path::new("scripts").join(script_file);
        let script_full_path = self.base_dir.join(&script_rel_path);

        if !script_full_path.exists() {
             return Err(Error::tool_execution(
                 self.name(),
                 format!("Script not found at {:?}", script_full_path),
             ).into());
        }
        
        info!(tool = %self.name(), "Executing dynamic skill (Runtime: {})", runtime_type);

        let has_bwrap = which::which("bwrap").is_ok();
        let unsafe_override = std::env::var("AAGT_UNSAFE_SKILL_EXEC").map(|v| v == "true").unwrap_or(false);
        let timeout = std::time::Duration::from_secs(self.execution_config.timeout_secs);

        if !has_bwrap && !unsafe_override {
             return Err(Error::tool_execution(
                  self.name(), 
                 "Security Error: 'bwrap' (Bubblewrap) sandbox is not installed on the system. Cannot execute skill securely. \
                 To bypass this for testing, set AAGT_UNSAFE_SKILL_EXEC=true."
             ).into());
        }

        let output = if !has_bwrap && unsafe_override {
            warn!(tool = %self.name(), "UNSAFE EXECUTION: Running skill without Bubblewrap sandbox override.");
            let mut cmd = tokio::process::Command::new(interpreter);
            cmd.arg(&script_full_path);
            cmd.arg(arguments);
            cmd.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
            for (key, value) in &self.execution_config.env_vars { cmd.env(key, value); }
            
            let child = cmd.spawn()
                .map_err(|e| Error::ToolExecution { 
                    tool_name: self.name(), 
                    message: format!("Failed to spawn process: {}", e) 
                })?;

            tokio::time::timeout(timeout, child.wait_with_output())
                .await
                .map_err(|_| Error::ToolExecution { 
                    tool_name: self.name(), 
                    message: "Execution timed out".to_string() 
                })?
                .map_err(|e| Error::ToolExecution { 
                    tool_name: self.name(), 
                    message: format!("Process failed: {}", e) 
                })?
        } else {
            let mut cmd = tokio::process::Command::new("bwrap");
            cmd.arg("--ro-bind").arg("/").arg("/");
            cmd.arg("--dev").arg("/dev");
            cmd.arg("--proc").arg("/proc");
            cmd.arg("--tmpfs").arg("/tmp");
            if let Ok(cwd) = std::env::current_dir() {
                cmd.arg("--bind").arg(&cwd).arg(&cwd);
            }
            if !self.execution_config.allow_network {
                cmd.arg("--unshare-net");
            }
            cmd.arg(interpreter).arg(&script_full_path).arg(arguments);
            cmd.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
            for (key, value) in &self.execution_config.env_vars { cmd.env(key, value); }

            let child = cmd.spawn()
                .map_err(|e| Error::ToolExecution { 
                    tool_name: self.name(), 
                    message: format!("Failed to spawn process: {}", e) 
                })?;

            tokio::time::timeout(timeout, child.wait_with_output())
                .await
                .map_err(|_| Error::ToolExecution { 
                    tool_name: self.name(), 
                    message: "Execution timed out".to_string() 
                })?
                .map_err(|e| Error::ToolExecution { 
                    tool_name: self.name(), 
                    message: format!("Process failed: {}", e) 
                })?
        };

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if !output.status.success() {
            return Err(Error::ToolExecution {
                tool_name: self.name(),
                message: format!("Script error (exit code {}): {}\nStderr: {}", 
                    output.status.code().unwrap_or(-1), stdout, stderr)
            }.into());
        }

        // 🛡️ Safety Check: Parse for Proposals (Trading logic)
        #[cfg(feature = "trading")]
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&stdout) {
            if value.get("type").and_then(|t| t.as_str()) == Some("proposal") {
                if let Some(proposal_data) = value.get("data") {
                    let proposal: Proposal = serde_json::from_value(proposal_data.clone())
                        .map_err(|e| Error::tool_execution(self.name(), format!("Malformed proposal: {}", e)))?;

                    info!("Skill {} generated a transaction proposal: {:?}", self.name(), proposal);

                    if let Some(ref rm) = self.risk_manager {
                        let context = crate::trading::risk::TradeContext {
                            user_id: "default_user".to_string(), 
                            from_token: proposal.from_token.clone(),
                            to_token: proposal.to_token.clone(),
                            amount_usd: proposal.amount_usd,
                            expected_slippage: proposal.expected_slippage.unwrap_or(rust_decimal_macros::dec!(1.0)),
                            liquidity_usd: None,
                            is_flagged: false,
                        };

                        rm.check_and_reserve(&context).await
                            .map_err(|e| Error::tool_execution(self.name(), format!("Risk Check Denied: {}", e)))?;

                        if let Some(ref executor) = self.executor {
                             let action = Action::Swap {
                                 from_token: proposal.from_token,
                                 to_token: proposal.to_token,
                                 amount: proposal.amount,
                             };
                             let pipeline_ctx = crate::trading::pipeline::Context::new(format!("Skill execution: {}", self.name()));
                             let result = match executor.execute(&action, &pipeline_ctx).await {
                                Ok(res) => res,
                                Err(e) => {
                                    warn!("Skill execution failed, rolling back risk reservation: {}", e);
                                    rm.rollback_trade(&context.user_id, context.amount_usd).await;
                                    return Err(Error::tool_execution(self.name(), format!("Execution Failed (Rolled Back): {}", e)).into());
                                }
                             };
                             rm.commit_trade(&context.user_id, context.amount_usd).await?;
                             return Ok(format!("SUCCESS: Trade executed: {}", result));
                        } else {
                            rm.commit_trade(&context.user_id, context.amount_usd).await?;
                            return Ok(format!("SIMULATION SUCCESS: Trade approved but NO EXECUTOR configured. Proposal: {:?}", proposal));
                        }
                    } else {
                        return Err(Error::tool_execution(self.name(), "RiskManager not configured, cannot execute risky proposal".to_string()).into());
                    }
                }
            }
        }

        Ok(stdout)
    }
}

/// Registry and loader for dynamic skills
pub struct SkillLoader {
    pub skills: DashMap<String, Arc<DynamicSkill>>,
    pub base_path: PathBuf,
    #[cfg(feature = "trading")]
    risk_manager: Option<Arc<RiskManager>>,
    #[cfg(feature = "trading")]
    executor: Option<Arc<dyn ActionExecutor>>,
}

impl SkillLoader {
    /// Create a new registry
    pub fn new(base_path: impl Into<PathBuf>) -> Self {
        Self {
            skills: DashMap::new(),
            base_path: base_path.into(),
            #[cfg(feature = "trading")]
            risk_manager: None,
            #[cfg(feature = "trading")]
            executor: None,
        }
    }

    /// Set a risk manager for all loaded skills
    #[cfg(feature = "trading")]
    pub fn with_risk_manager(mut self, risk_manager: Arc<RiskManager>) -> Self {
        self.risk_manager = Some(risk_manager);
        self
    }

    /// Set an executor for all loaded skills
    #[cfg(feature = "trading")]
    pub fn with_executor(mut self, executor: Arc<dyn ActionExecutor>) -> Self {
        self.executor = Some(executor);
        self
    }

    /// Load all skills from the base directory
    pub async fn load_all(&self) -> Result<()> {
        if !self.base_path.exists() {
            return Ok(());
        }

        let mut entries = tokio::fs::read_dir(&self.base_path).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_dir() {
                if let Ok(skill) = self.load_skill(&path).await {
                    #[cfg(feature = "trading")]
                    let mut skill = skill;
                    #[cfg(feature = "trading")]
                    {
                        if let Some(ref rm) = self.risk_manager {
                            skill = skill.with_risk_manager(Arc::clone(rm));
                        }
                        if let Some(ref exec) = self.executor {
                            skill = skill.with_executor(Arc::clone(exec));
                        }
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

        let mut metadata: SkillMetadata = serde_yaml_ng::from_str(yaml_str)
            .map_err(|e| Error::Internal(format!("Failed to parse Skill YAML: {}", e)))?;
        
        // --- 兼容性提升: 自动推断缺失的元数据 ---
        // 如果 frontmatter 缺失 script 或 runtime，尝试从 Markdown 正文中解析
        if metadata.script.is_none() || metadata.runtime.is_none() {
            // 查找常见的脚本执行模式，例如: ```bash \n python3 .../scripts/xxx.py \n ```
            // 或者直接查找 scripts/ 目录下的第一个文件
            if metadata.script.is_none() {
                let scripts_dir = path.join("scripts");
                if scripts_dir.exists() {
                    if let Ok(mut entries) = std::fs::read_dir(scripts_dir) {
                        if let Some(Ok(first_entry)) = entries.next() {
                            let filename = first_entry.file_name().to_string_lossy().to_string();
                            metadata.script = Some(filename.clone());
                            
                            // 根据后缀推断 runtime
                            if metadata.runtime.is_none() {
                                if filename.ends_with(".py") {
                                    metadata.runtime = Some("python3".into());
                                } else if filename.ends_with(".js") {
                                    metadata.runtime = Some("node".into());
                                } else if filename.ends_with(".sh") {
                                    metadata.runtime = Some("bash".into());
                                }
                            }
                        }
                    }
                }
            }

            // 如果还是没找到，尝试从代码块中解析执行指令
            if metadata.runtime.is_none() {
                if instructions.contains("python3") {
                    metadata.runtime = Some("python3".into());
                } else if instructions.contains("node") {
                    metadata.runtime = Some("node".into());
                } else if instructions.contains("bash") || instructions.contains("sh ") {
                    metadata.runtime = Some("bash".into());
                }
            }
        }
        
        Ok(DynamicSkill::new(metadata, instructions, path.to_path_buf()))
    }
}

#[async_trait::async_trait]
impl ContextInjector for SkillLoader {
    async fn inject(&self) -> Result<Vec<Message>> {
        if self.skills.is_empty() {
            return Ok(Vec::new());
        }

        let mut content = String::from("## Available Skills\n\n");
        content.push_str("You have the following skills available via `read_skill_manual`:\n\n");

        for skill_ref in self.skills.iter() {
            let skill = skill_ref.value();
            content.push_str(&format!("- **{}**: {}\n", skill.name(), skill.metadata.description));
        }

        content.push_str("\nUse `read_skill_manual(skill_name)` to see full instructions for any skill.\n");

        Ok(vec![Message::system(content)])
    }
}

/// Tool to read the full SKILL.md guide for a specific skill
pub struct ReadSkillDoc {
    loader: Arc<SkillLoader>,
}

impl ReadSkillDoc {
    pub fn new(loader: Arc<SkillLoader>) -> Self {
        Self { loader }
    }
}

#[async_trait]
impl Tool for ReadSkillDoc {
    fn name(&self) -> String {
        "read_skill_manual".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Read the full SKILL.md manual for a specific skill to understand its parameters and usage examples.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "skill_name": {
                        "type": "string",
                        "description": "The name of the skill to read documentation for"
                    }
                },
                "required": ["skill_name"]
            }),
            parameters_ts: Some("interface ReadSkillArgs {\n  skill_name: string; // The name of the skill to read manual for\n}".to_string()),
            is_binary: false,
            is_verified: true,
            usage_guidelines: None,
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            skill_name: String,
        }
        let args: Args = serde_json::from_str(arguments)?;
        
        if let Some(skill) = self.loader.skills.get(&args.skill_name) {
            Ok(format!("# Skill: {}\n\n{}", skill.name(), skill.instructions))
        } else {
            Err(anyhow::anyhow!("Skill '{}' not found in registry", args.skill_name))
        }
    }
}
/// Tool to search and install skills from ClawHub using CLI (npm/pnpm/bun)
pub struct ClawHubTool {
    loader: Arc<SkillLoader>,
}

impl ClawHubTool {
    pub fn new(loader: Arc<SkillLoader>) -> Self {
        Self { loader }
    }
}

#[async_trait]
impl Tool for ClawHubTool {
    fn name(&self) -> String {
        "clawhub_manager".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Search and install new skills from the ClawHub.ai registry. Supports 'search' to find skills and 'install' to add them to your environment.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["search", "install"],
                        "description": "The action to perform"
                    },
                    "query": {
                        "type": "string",
                        "description": "Search query or skill slug to install"
                    },
                    "manager": {
                        "type": "string",
                        "enum": ["npm", "pnpm", "bun"],
                        "description": "The package manager to use (default: npm)"
                    }
                },
                "required": ["action", "query"]
            }),
            parameters_ts: Some("interface ClawHubArgs {\n  action: 'search' | 'install';\n  query: string; // Search query or skill slug\n  manager?: 'npm' | 'pnpm' | 'bun'; // Package manager (default: npm)\n}".to_string()),
            is_binary: false,
            is_verified: true,
            usage_guidelines: None,
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            action: String,
            query: String,
            manager: Option<String>,
        }
        let args: Args = serde_json::from_str(arguments)?;

        let manager = args.manager.as_deref().unwrap_or("npm");
        let (cmd, base_args) = match manager {
            "pnpm" => ("pnpm", vec!["dlx", "clawhub@latest"]),
            "bun" => ("bunx", vec!["clawhub@latest"]),
            _ => ("npx", vec!["clawhub@latest"]),
        };

        match args.action.as_str() {
            "search" => {
                info!("Searching ClawHub registry for: {} (via {})", args.query, manager);
                let output = tokio::process::Command::new(cmd)
                    .args(&base_args)
                    .arg("search")
                    .arg(&args.query)
                    .output()
                    .await?;
                
                Ok(String::from_utf8_lossy(&output.stdout).to_string())
            }
            "install" => {
                info!("Installing skill from ClawHub: {} (via {})", args.query, manager);
                let output = tokio::process::Command::new(cmd)
                    .args(&base_args)
                    .arg("install")
                    .arg(&args.query)
                    .output()
                    .await?;

                if output.status.success() {
                    // Refresh the loader to pick up the new skill
                    info!("Skill {} installed successfully, refreshing registry...", args.query);
                    self.loader.load_all().await?;
                    Ok(format!("Successfully installed '{}'. It is now available for use.", args.query))
                } else {
                    let err = String::from_utf8_lossy(&output.stderr);
                    Err(anyhow::anyhow!("Failed to install skill: {}", err))
                }
            }
            _ => Err(anyhow::anyhow!("Unknown action: {}", args.action)),
        }
    }
}
