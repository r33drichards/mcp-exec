use std::collections::BTreeMap;
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

use crate::executor::{new_task_registry, spawn_command, CommandSpec, TaskRegistry};
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
//
// `exec` takes a program and its arguments, not a command line: nothing here is
// given to a shell. Unknown fields are refused, so a request in the old form
// (`cmd`) fails with an error that names the fields that exist.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExecRequest {
    #[schemars(description = "The program to run: a name looked up on PATH, or a path. Executed directly, not by a shell")]
    pub bin: String,
    #[serde(default)]
    #[schemars(description = "The program's arguments, passed as they are: no word splitting, globbing or expansion (default: none)")]
    pub args: Vec<String>,
    #[schemars(description = "Timeout in seconds; then the program and everything it started are killed")]
    pub timeout: u64,
    #[serde(default)]
    #[schemars(description = "Working directory: an absolute path (default: the server's working directory)")]
    pub cwd: Option<String>,
    #[serde(default)]
    #[schemars(description = "Environment variables to add to the server's environment")]
    pub env: Option<BTreeMap<String, String>>,
}

impl ExecRequest {
    /// Check the request and turn it into what the executor runs.
    fn into_spec(self) -> Result<CommandSpec, String> {
        if self.bin.is_empty() {
            return Err("bin must not be empty".to_string());
        }
        if self.bin.contains('\0') || self.args.iter().any(|a| a.contains('\0')) {
            return Err("bin and args must not contain NUL characters".to_string());
        }
        if let Some(cwd) = &self.cwd {
            if !std::path::Path::new(cwd).is_absolute() || cwd.contains('\0') {
                return Err(format!("cwd must be an absolute path: {:?}", cwd));
            }
            if !std::path::Path::new(cwd).is_dir() {
                return Err(format!("cwd is not a directory: {:?}", cwd));
            }
        }
        let env = self.env.unwrap_or_default();
        for (name, value) in &env {
            if name.is_empty() || name.contains('=') || name.contains('\0') {
                return Err(format!("env: {:?} is not a variable name", name));
            }
            if value.contains('\0') {
                return Err(format!("env: the value of {} must not contain NUL characters", name));
            }
        }
        Ok(CommandSpec { bin: self.bin, args: self.args, cwd: self.cwd, env })
    }
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

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct KillRequest {
    #[schemars(description = "The execution UUID")]
    pub id: String,
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
    async fn start_execution(&self, id: Uuid, spec: CommandSpec, timeout_secs: u64) -> Result<(), String> {
        // What is kept about the command: the program and its arguments, as JSON.
        let described = serde_json::json!({ "bin": spec.bin, "args": spec.args, "cwd": spec.cwd }).to_string();
        self.store.create(id, described).await?;
        let cancel = tokio_util::sync::CancellationToken::new();
        self.tasks.lock().await.insert(id, cancel.clone());
        let _ = spawn_command(self.store.clone(), self.tasks.clone(), cancel, id, spec, timeout_secs);
        Ok(())
    }

    /// How many commands are running (registered and not yet ended).
    #[cfg(test)]
    pub async fn running(&self) -> usize {
        self.tasks.lock().await.len()
    }

    /// Stop a running execution: cancel it and record the terminal status at once, so that
    /// it stays cancelled even if the command races to completion.
    async fn cancel_execution(&self, id: Uuid) {
        if let Some(token) = self.tasks.lock().await.get(&id) {
            token.cancel();
        }
        let _ = self.store.set_status(id, ExecutionStatus::Cancelled).await;
    }

    /// Run a program asynchronously. Returns immediately with a UUID to track the execution.
    #[tool(
        description = "Run a program asynchronously: `bin` with the arguments `args`, executed directly (no shell: no pipes, globbing or variable expansion; for those run bin \"sh\" with args [\"-c\", \"...\"]). Optional `cwd` and `env`. Returns a UUID to track execution. Use stream_logs to get output and kill to stop it. Task-capable clients may invoke this as a task (tasks/get, tasks/result, tasks/cancel). The `cmd` field of earlier versions no longer exists.",
        execution(task_support = "optional")
    )]
    pub async fn exec(&self, Parameters(req): Parameters<ExecRequest>) -> Result<String, McpError> {
        let id = Uuid::new_v4();
        let timeout = req.timeout;
        let spec = req.into_spec().map_err(|e| McpError::invalid_params(e, None))?;

        if let Err(e) = self.start_execution(id, spec, timeout).await {
            return Ok(serde_json::to_string(&ExecResponse {
                id: id.to_string(),
                status: format!("error: {}", e),
            })
            .unwrap_or_else(|_| format!("{{\"id\":\"{}\",\"status\":\"error\"}}", id)));
        }

        Ok(serde_json::to_string(&ExecResponse {
            id: id.to_string(),
            status: "started".to_string(),
        })
        .unwrap_or_else(|_| format!("{{\"id\":\"{}\",\"status\":\"started\"}}", id)))
    }

    /// Stop a running execution, with everything it started.
    #[tool(description = "Stop a running execution: the program and everything it started are killed and its status becomes \"cancelled\". Returns the execution's status; an execution that had already ended keeps the status it had.")]
    pub async fn kill(&self, Parameters(req): Parameters<KillRequest>) -> String {
        let respond = |status: String| {
            serde_json::to_string(&ExecResponse { id: req.id.clone(), status })
                .unwrap_or_else(|_| "{\"status\":\"error\"}".to_string())
        };
        let uuid = match Uuid::parse_str(&req.id) {
            Ok(u) => u,
            Err(e) => return respond(format!("error: Invalid UUID: {}", e)),
        };
        let status = match self.store.get_status(uuid).await {
            Ok(s) => s,
            Err(e) => return respond(format!("error: {}", e)),
        };
        if is_terminal(&status) {
            return respond(status_to_string(&status));
        }
        self.cancel_execution(uuid).await;
        respond(status_to_string(&ExecutionStatus::Cancelled))
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
            "Command execution service with log streaming and search: `exec` runs a program \
             with arguments (no shell), `stream_logs` and `search_logs` read its output, \
             `kill` stops it. Supports MCP tasks (SEP-1686): the `exec` tool may be invoked \
             as a task, with the task tracking the command's lifecycle (tasks/get, \
             tasks/result, tasks/cancel)."
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
        let timeout = req.timeout;
        let spec = req.into_spec().map_err(|e| McpError::invalid_params(e, None))?;
        self.start_execution(id, spec, timeout)
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

        // Enforces the terminal cancelled state immediately: per spec a cancelled task must
        // remain cancelled even if execution races to completion.
        self.cancel_execution(id).await;

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
