//! Execution Pipelines for AAGT
//!
//! A pipeline is a sequence of steps that process data and make decisions.
//! This is useful for structuring complex workflows like:
//! 1. Data Collection -> 2. Analysis -> 3. Risk Check -> 4. Execution
//!
//! Unlike simple chains, access to a shared `Context` allows steps to pass data efficiently
//! and mix AI agents with hard-coded logic (e.g. risk checks).

use std::collections::HashMap;
use std::sync::Arc;
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

/// Shared execution context passed between steps
#[derive(Debug, Default, Clone)]
pub struct Context {
    /// Initial user input or trigger
    pub input: String,
    /// Shared key-value store for inter-step communication
    pub data: HashMap<String, Value>,
    /// Execution logs/trace
    pub trace: Vec<String>,
    /// Whether the pipeline should abort execution
    pub aborted: bool,
    /// Final result/decision of the pipeline
    pub outcome: Option<String>,
}

impl Context {
    /// Create new context
    pub fn new(input: impl Into<String>) -> Self {
        Self {
            input: input.into(),
            data: HashMap::new(),
            trace: Vec::new(),
            aborted: false,
            outcome: None,
        }
    }

    /// Set a value in the context
    pub fn set(&mut self, key: &str, value: impl Into<Value>) {
        self.data.insert(key.to_string(), value.into());
    }

    /// Get a value from the context
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.data.get(key)
    }

    /// Abort the pipeline with a reason
    pub fn abort(&mut self, reason: &str) {
        self.aborted = true;
        self.log(format!("ABORTED: {}", reason));
    }

    /// Add a log entry
    pub fn log(&mut self, message: impl Into<String>) {
        self.trace.push(message.into());
    }
}

/// A single step in the pipeline
#[async_trait]
pub trait Step: Send + Sync {
    /// Execute this step
    async fn execute(&self, ctx: &mut Context) -> Result<()>;
    
    /// Name of the step for debugging
    fn name(&self) -> &str;
}

/// Linear execution pipeline
pub struct Pipeline {
    /// Steps to execute
    steps: Vec<Box<dyn Step>>,
    /// Name of this pipeline
    name: String,
}

impl Pipeline {
    /// Create a new pipeline
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            steps: Vec::new(),
            name: name.into(),
        }
    }

    /// Add a step to the pipeline
    pub fn add_step(mut self, step: impl Step + 'static) -> Self {
        self.steps.push(Box::new(step));
        self
    }

    /// Execute the pipeline
    pub async fn run(&self, input: impl Into<String>) -> Result<Context> {
        let mut ctx = Context::new(input);
        
        ctx.log(format!("Pipeline '{}' started", self.name));

        for step in &self.steps {
            if ctx.aborted {
                ctx.log("Skipping remaining steps due to abort");
                break;
            }

            ctx.log(format!("Running step: {}", step.name()));
            
            // Execute the step
            if let Err(e) = step.execute(&mut ctx).await {
                ctx.log(format!("ERROR in {}: {}", step.name(), e));
                return Err(e);
            }
        }

        ctx.log(format!("Pipeline '{}' finished", self.name));
        Ok(ctx)
    }
}

// --- Example Implementation Helpers ---

/// A simple closure-based step
pub struct LambdaStep<F> {
    name: String,
    func: F,
}

impl<F> LambdaStep<F> {
    pub fn new(name: impl Into<String>, func: F) -> Self {
        Self {
            name: name.into(),
            func,
        }
    }
}

#[async_trait]
impl<F, Fut> Step for LambdaStep<F>
where
    F: Fn(&mut Context) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = Result<()>> + Send,
{
    async fn execute(&self, ctx: &mut Context) -> Result<()> {
        (self.func)(ctx).await
    }

    fn name(&self) -> &str {
        &self.name
    }
}
