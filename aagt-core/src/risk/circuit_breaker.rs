//! Circuit breaker mechanisms for risk control

use crate::risk::{RiskCheck, RiskCheckResult, TradeContext};
use std::path::PathBuf;

/// A "Dead Man's Switch" that blocks all trades if a specific file exists.
///
/// This is useful for emergency shutdowns without needing SSH access or process killing.
/// Just creating a file (e.g. via FTP/SFTP or a simple dashboard) triggers this check.
#[derive(Debug, Clone)]
pub struct DeadManSwitch {
    /// Path to the stop file
    path: PathBuf,
}

impl DeadManSwitch {
    /// Create a new switch watching the given path
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

impl RiskCheck for DeadManSwitch {
    fn name(&self) -> &str {
        "dead_man_switch"
    }

    fn check(&self, _context: &TradeContext) -> RiskCheckResult {
        if self.path.exists() {
            RiskCheckResult::Rejected {
                reason: format!("EMERGENCY STOP: File {:?} detected.", self.path),
            }
        } else {
            RiskCheckResult::Approved
        }
    }
}
