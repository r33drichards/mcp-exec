use std::time::Duration;

use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{
        CallToolRequestParams, CallToolResult, CancelTaskParams, CancelTaskResult, Content,
        CreateTaskResult, ErrorCode, GetTaskInfoParams, GetTaskPayloadResult, GetTaskResult,
        GetTaskResultParams, ListTasksResult, PaginatedRequestParams, ServerCapabilities,
        ServerInfo, Task, TasksCapability, TaskStatus,
    },
    schemars,
    service::RequestContext,
    task_manager::current_timestamp,
    tool, tool_handler, tool_router, ErrorData as McpError, RoleServer, ServerHandler,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::executor::{new_task_registry, spawn_command, TaskRegistry};
use crate::log_store::{AnyLogStore, LogStore};
use crate::types::{ExecutionStatus, LogMatch};

/// Suggested polling interval (milliseconds) advertised to task requestors.
const TASK_POLL_INTERVAL_MS: u64 = 1000;
/// How often `tasks/result` re-checks a still-running execution.
const RESULT_POLL_INTERVAL_MS: u64 = 250;

#[derive(Clone)]
pub struct ExecService {
    store: AnyLogStore,
    tasks: TaskRegistry,
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
        ExecutionStatus::Cancelled => "cancelled".to_string(),
    }
}

/// Map an internal execution status onto the MCP task lifecycle status.
fn to_task_status(status: &ExecutionStatus) -> TaskStatus {
    match status {
        ExecutionStatus::Running => TaskStatus::Working,
        ExecutionStatus::Completed(_) => TaskStatus::Completed,
        ExecutionStatus::Failed(_) | ExecutionStatus::Timeout => TaskStatus::Failed,
        ExecutionStatus::Cancelled => TaskStatus::Cancelled,
    }
}

fn is_terminal(status: &ExecutionStatus) -> bool {
    !matches!(status, ExecutionStatus::Running)
}

/// Build a spec-compliant [`Task`] snapshot from an execution's current status.
fn build_task(id: Uuid, status: &ExecutionStatus) -> Task {
    let now = current_timestamp();
    Task::new(id.to_string(), to_task_status(status), now.clone(), now)
        .with_status_message(status_to_string(status))
        .with_poll_interval(TASK_POLL_INTERVAL_MS)
}

fn parse_task_id(task_id: &str) -> Result<Uuid, McpError> {
    Uuid::parse_str(task_id)
        .map_err(|_| McpError::invalid_params(format!("invalid task id: {}", task_id), None))
}

#[tool_router]
impl ExecService {
    pub fn new(store: AnyLogStore) -> Self {
        Self {
            store,
            tasks: new_task_registry(),
            tool_router: Self::tool_router(),
        }
    }

    /// Create the execution record, register a cancellation token, and spawn the command.
    async fn start_execution(&self, id: Uuid, cmd: String, timeout_secs: u64) -> Result<(), String> {
        self.store.create(id, cmd.clone()).await?;
        let cancel = tokio_util::sync::CancellationToken::new();
        self.tasks.lock().await.insert(id, cancel.clone());
        let _ = spawn_command(self.store.clone(), self.tasks.clone(), cancel, id, cmd, timeout_secs);
        Ok(())
    }

    /// Execute a shell command asynchronously. Returns immediately with a UUID to track the execution.
    #[tool(
        description = "Execute a shell command asynchronously. Returns a UUID to track execution. Use stream_logs to get output. Task-capable clients may invoke this as a task (tasks/get, tasks/result, tasks/cancel).",
        execution(task_support = "optional")
    )]
    pub async fn exec(&self, Parameters(req): Parameters<ExecRequest>) -> String {
        let id = Uuid::new_v4();

        if let Err(e) = self.start_execution(id, req.cmd, req.timeout).await {
            return serde_json::to_string(&ExecResponse {
                id: id.to_string(),
                status: format!("error: {}", e),
            })
            .unwrap_or_else(|_| format!("{{\"id\":\"{}\",\"status\":\"error\"}}", id));
        }

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
        let mut info = ServerInfo::default();
        info.instructions = Some(
            "Shell command execution service with log streaming and search. Supports MCP \
             tasks (SEP-1686): the `exec` tool may be invoked as a task, with the task \
             tracking the command's lifecycle (tasks/get, tasks/result, tasks/cancel)."
                .into(),
        );
        info.capabilities = ServerCapabilities::builder()
            .enable_tools()
            .enable_tasks_with(TasksCapability::server_default())
            .build();
        info
    }

    /// Enqueue a task-augmented `tools/call`. Only `exec` supports task execution; the task
    /// id is the execution UUID, so it can be used with stream_logs/search_logs as well.
    async fn enqueue_task(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CreateTaskResult, McpError> {
        if request.name.as_ref() != "exec" {
            return Err(McpError::new(
                ErrorCode::METHOD_NOT_FOUND,
                format!("tool '{}' does not support task-based invocation", request.name),
                None,
            ));
        }

        let arguments = request.arguments.clone().unwrap_or_default();
        let req: ExecRequest = serde_json::from_value(serde_json::Value::Object(arguments))
            .map_err(|e| McpError::invalid_params(format!("invalid exec arguments: {}", e), None))?;

        let id = Uuid::new_v4();
        self.start_execution(id, req.cmd, req.timeout)
            .await
            .map_err(|e| McpError::internal_error(format!("failed to start execution: {}", e), None))?;

        let ttl = request
            .task
            .as_ref()
            .and_then(|t| t.get("ttl"))
            .and_then(|v| v.as_u64());

        let now = current_timestamp();
        let mut task = Task::new(id.to_string(), TaskStatus::Working, now.clone(), now)
            .with_status_message("command started")
            .with_poll_interval(TASK_POLL_INTERVAL_MS);
        if let Some(ttl) = ttl {
            task = task.with_ttl(ttl);
        }

        Ok(CreateTaskResult::new(task))
    }

    /// Report current task status (`tasks/get`).
    async fn get_task_info(
        &self,
        request: GetTaskInfoParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetTaskResult, McpError> {
        let id = parse_task_id(&request.task_id)?;
        let status = self
            .store
            .get_status(id)
            .await
            .map_err(|_| McpError::invalid_params(format!("task not found: {}", request.task_id), None))?;
        Ok(GetTaskResult {
            meta: None,
            task: build_task(id, &status),
        })
    }

    /// Block until the command reaches a terminal state, then return its result (`tasks/result`).
    async fn get_task_result(
        &self,
        request: GetTaskResultParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetTaskPayloadResult, McpError> {
        let id = parse_task_id(&request.task_id)?;

        loop {
            let status = self
                .store
                .get_status(id)
                .await
                .map_err(|_| McpError::invalid_params(format!("task not found: {}", request.task_id), None))?;

            if is_terminal(&status) {
                let logs = self
                    .store
                    .read(id, 0)
                    .await
                    .map(|(bytes, _)| String::from_utf8_lossy(&bytes).to_string())
                    .unwrap_or_default();

                let summary = status_to_string(&status);
                let text = if logs.is_empty() {
                    format!("[status: {}]", summary)
                } else {
                    format!("{}\n[status: {}]", logs.trim_end_matches('\n'), summary)
                };

                let result = match status {
                    ExecutionStatus::Completed(_) => CallToolResult::success(vec![Content::text(text)]),
                    _ => CallToolResult::error(vec![Content::text(text)]),
                };
                let value = serde_json::to_value(&result)
                    .map_err(|e| McpError::internal_error(e.to_string(), None))?;
                return Ok(GetTaskPayloadResult::new(value));
            }

            tokio::time::sleep(Duration::from_millis(RESULT_POLL_INTERVAL_MS)).await;
        }
    }

    /// Cancel a running command (`tasks/cancel`).
    async fn cancel_task(
        &self,
        request: CancelTaskParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CancelTaskResult, McpError> {
        let id = parse_task_id(&request.task_id)?;
        let status = self
            .store
            .get_status(id)
            .await
            .map_err(|_| McpError::invalid_params(format!("task not found: {}", request.task_id), None))?;

        if is_terminal(&status) {
            return Err(McpError::invalid_params(
                format!("cannot cancel task in terminal status: {}", status_to_string(&status)),
                None,
            ));
        }

        if let Some(token) = self.tasks.lock().await.get(&id) {
            token.cancel();
        }
        // Enforce the terminal cancelled state immediately, per spec a cancelled task must
        // remain cancelled even if execution races to completion.
        let _ = self.store.set_status(id, ExecutionStatus::Cancelled).await;

        let now = current_timestamp();
        let task = Task::new(id.to_string(), TaskStatus::Cancelled, now.clone(), now)
            .with_status_message("task cancelled by request");
        Ok(CancelTaskResult { meta: None, task })
    }

    /// List all known executions as tasks (`tasks/list`).
    async fn list_tasks(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListTasksResult, McpError> {
        let executions = self
            .store
            .list_executions()
            .await
            .map_err(|e| McpError::internal_error(e, None))?;

        let total = executions.len() as u64;
        let tasks = executions
            .into_iter()
            .map(|(id, status)| build_task(id, &status))
            .collect();

        let mut result = ListTasksResult::new(tasks);
        result.total = Some(total);
        Ok(result)
    }
}
