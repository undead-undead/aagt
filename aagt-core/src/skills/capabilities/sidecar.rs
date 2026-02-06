//! gRPC Client for the Python Sidecar

use tonic::transport::Channel;
use crate::error::{Error, Result};

// Include the generated gRPC code
pub mod proto {
    tonic::include_proto!("sidecar");
}

use proto::sidecar_client::SidecarClient;
use proto::{ExecuteRequest, ExecuteResponse};

/// A client for interacting with the Python sidecar
pub struct Sidecar {
    client: SidecarClient<Channel>,
}

impl Sidecar {
    /// Connect to the sidecar at the given address
    pub async fn connect(dst: String) -> Result<Self> {
        let client = SidecarClient::connect(dst).await
            .map_err(|e| Error::Internal(format!("Failed to connect to sidecar: {}", e)))?;
        Ok(Self { client })
    }

    /// Execute Python code in the sidecar
    pub async fn execute(&mut self, code: String) -> Result<ExecuteResponse> {
        let request = tonic::Request::new(ExecuteRequest { code });
        let response = self.client.execute(request).await
            .map_err(|e| Error::ToolExecution {
                tool_name: "code_interpreter".to_string(),
                message: format!("Sidecar gRPC error: {}", e),
            })?;
        Ok(response.into_inner())
    }
}
