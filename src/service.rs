use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router, ServerHandler,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::executor::spawn_command;
use crate::log_store::{AnyLogStore, LogStore};
use crate::types::{ExecutionStatus, LogMatch};

#[derive(Clone)]
pub struct ExecService {
    store: AnyLogStore,
    tool_router: ToolRouter<Self>,
}

// Request types for tool parameters
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct ExecRequest {
    #[schemars(description = "The shell command to execute")]
    pub cmd: String,
    #[schemars(description = "Timeout in seconds")]
    pub timeout: u64,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct StreamLogsRequest {
    #[schemars(description = "The execution UUID")]
    pub id: String,
    #[schemars(description = "Byte offset to start reading from")]
    pub offset: u64,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct SearchLogsRequest {
    #[schemars(description = "The execution UUID")]
    pub id: String,
    #[schemars(description = "Regex pattern to search for")]
    pub pattern: String,
}

// Response types
#[derive(Debug, Clone, Serialize)]
pub struct ExecResponse {
    pub id: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StreamLogsResponse {
    pub logs: String,
    pub next_offset: u64,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchLogsResponse {
    pub matches: Vec<LogMatch>,
}

fn status_to_string(status: &ExecutionStatus) -> String {
    match status {
        ExecutionStatus::Running => "running".to_string(),
        ExecutionStatus::Completed(code) => format!("completed:{}", code),
        ExecutionStatus::Failed(msg) => format!("failed:{}", msg),
        ExecutionStatus::Timeout => "timeout".to_string(),
    }
}

#[tool_router]
impl ExecService {
    pub fn new(store: AnyLogStore) -> Self {
        Self {
            store,
            tool_router: Self::tool_router(),
        }
    }

    /// Execute a shell command asynchronously. Returns immediately with a UUID to track the execution.
    #[tool(description = "Execute a shell command asynchronously. Returns a UUID to track execution. Use stream_logs to get output.")]
    pub async fn exec(&self, Parameters(req): Parameters<ExecRequest>) -> String {
        let id = Uuid::new_v4();

        if let Err(e) = self.store.create(id, req.cmd.clone()).await {
            return serde_json::to_string(&ExecResponse {
                id: id.to_string(),
                status: format!("error: {}", e),
            })
            .unwrap_or_else(|_| format!("{{\"id\":\"{}\",\"status\":\"error\"}}", id));
        }

        let _ = spawn_command(self.store.clone(), id, req.cmd, req.timeout);

        serde_json::to_string(&ExecResponse {
            id: id.to_string(),
            status: "started".to_string(),
        })
        .unwrap_or_else(|_| format!("{{\"id\":\"{}\",\"status\":\"started\"}}", id))
    }

    /// Stream logs from an execution starting at the given byte offset.
    #[tool(description = "Stream logs from an execution. Returns logs from offset, next_offset for pagination, and current status.")]
    pub async fn stream_logs(&self, Parameters(req): Parameters<StreamLogsRequest>) -> String {
        let uuid = match Uuid::parse_str(&req.id) {
            Ok(u) => u,
            Err(e) => {
                return serde_json::to_string(&StreamLogsResponse {
                    logs: format!("Invalid UUID: {}", e),
                    next_offset: 0,
                    status: "error".to_string(),
                })
                .unwrap_or_else(|_| "{\"status\":\"error\"}".to_string());
            }
        };

        let (bytes, next_offset) = match self.store.read(uuid, req.offset).await {
            Ok(r) => r,
            Err(e) => {
                return serde_json::to_string(&StreamLogsResponse {
                    logs: format!("Error reading logs: {}", e),
                    next_offset: req.offset,
                    status: "error".to_string(),
                })
                .unwrap_or_else(|_| "{\"status\":\"error\"}".to_string());
            }
        };

        let status = match self.store.get_status(uuid).await {
            Ok(s) => status_to_string(&s),
            Err(e) => format!("error: {}", e),
        };

        serde_json::to_string(&StreamLogsResponse {
            logs: String::from_utf8_lossy(&bytes).to_string(),
            next_offset,
            status,
        })
        .unwrap_or_else(|_| "{\"status\":\"error\"}".to_string())
    }

    /// Search logs for a regex pattern. Returns matching lines with their byte offsets.
    #[tool(description = "Search logs for a regex pattern. Returns matching lines with byte offsets.")]
    pub async fn search_logs(&self, Parameters(req): Parameters<SearchLogsRequest>) -> String {
        let uuid = match Uuid::parse_str(&req.id) {
            Ok(u) => u,
            Err(_) => {
                return serde_json::to_string(&SearchLogsResponse { matches: vec![] })
                    .unwrap_or_else(|_| "{\"matches\":[]}".to_string());
            }
        };

        match self.store.search(uuid, &req.pattern).await {
            Ok(matches) => serde_json::to_string(&SearchLogsResponse { matches })
                .unwrap_or_else(|_| "{\"matches\":[]}".to_string()),
            Err(_) => serde_json::to_string(&SearchLogsResponse { matches: vec![] })
                .unwrap_or_else(|_| "{\"matches\":[]}".to_string()),
        }
    }
}

#[tool_handler]
impl ServerHandler for ExecService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some("Shell command execution service with log streaming and search".into()),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}
