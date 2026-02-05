//! Sidecar Process Manager

use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use tokio::sync::Mutex;
use crate::error::{Error, Result};

/// Configuration for the Sidecar process
#[derive(Debug, Clone)]
pub struct SidecarConfig {
    /// Path to the Python executable
    pub python_path: String,
    /// Path to the sidecar script
    pub script_path: String,
    /// Port of the sidecar server
    pub port: u16,
}

impl Default for SidecarConfig {
    fn default() -> Self {
        Self {
            python_path: "python3".to_string(),
            script_path: "aagt-sidecar/sidecar.py".to_string(),
            port: 50051,
        }
    }
}

/// Manages the lifecycle of the Python sidecar process
pub struct SidecarManager {
    config: SidecarConfig,
    child: Arc<Mutex<Option<Child>>>,
}

impl SidecarManager {
    /// Create a new SidecarManager
    pub fn new(config: SidecarConfig) -> Self {
        Self {
            config,
            child: Arc::new(Mutex::new(None)),
        }
    }

    /// Start the sidecar process
    pub async fn start(&self) -> Result<()> {
        let mut child_guard = self.child.lock().await;
        if child_guard.is_some() {
            return Ok(()); // Already running
        }

        let child = Command::new(&self.config.python_path)
            .arg(&self.config.script_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| Error::Internal(format!("Failed to spawn sidecar: {}", e)))?;

        *child_guard = Some(child);
        
        // Wait for server to be up (simple sleep for now, can be improved with health checks)
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        
        Ok(())
    }

    /// Stop the sidecar process
    pub async fn stop(&self) -> Result<()> {
        let mut child_guard = self.child.lock().await;
        if let Some(mut child) = child_guard.take() {
            child.kill()
                .map_err(|e| Error::Internal(format!("Failed to kill sidecar: {}", e)))?;
        }
        Ok(())
    }
}

impl Drop for SidecarManager {
    fn drop(&mut self) {
        // We can't really await here, but we can try to kill it if it's still running
        if let Ok(mut child_guard) = self.child.try_lock() {
            if let Some(mut child) = child_guard.take() {
                let _ = child.kill();
            }
        }
    }
}
