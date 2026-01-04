# MCP Exec Server Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build an MCP server that executes shell commands asynchronously with log streaming and search capabilities.

**Architecture:** Rust MCP server using `rmcp` crate. Three tools: `exec` (spawns command, returns UUID), `stream_logs` (offset-based log reading), `search_logs` (regex search). Log storage abstracted via trait with in-memory and file-based implementations.

**Tech Stack:** Rust, rmcp, tokio, clap, uuid, regex, serde

---

### Task 1: Project Setup

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`

**Step 1: Create Cargo.toml**

```toml
[package]
name = "mcp-exec"
version = "0.1.0"
edition = "2021"

[dependencies]
anyhow = "1.0"
axum = "0.8"
rmcp = { git = "https://github.com/modelcontextprotocol/rust-sdk", branch = "main", features = ["transport-io", "transport-sse-server"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tokio = { version = "1", features = ["rt-multi-thread", "process", "signal", "sync", "time", "io-util"] }
tracing = "0.1"
tracing-subscriber = "0.3"
clap = { version = "4", features = ["derive"] }
uuid = { version = "1", features = ["v4"] }
regex = "1"
async-trait = "0.1"
hyper = { version = "1", features = ["server", "http1"] }
hyper-util = { version = "0.1", features = ["tokio"] }
tokio-util = "0.7"

[dev-dependencies]
tokio-test = "0.4"
```

**Step 2: Create minimal main.rs**

```rust
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    println!("mcp-exec server");
    Ok(())
}
```

**Step 3: Verify build**

Run: `cargo build`
Expected: Compiles successfully (may take a while to fetch rmcp)

**Step 4: Commit**

```bash
git add Cargo.toml src/main.rs
git commit -m "feat: initialize mcp-exec project with dependencies"
```

---

### Task 2: Log Store Trait and Types

**Files:**
- Create: `src/log_store/mod.rs`
- Create: `src/types.rs`
- Modify: `src/main.rs`

**Step 1: Create types.rs with ExecutionStatus**

```rust
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExecutionStatus {
    Running,
    Completed(i32),
    Failed(String),
    Timeout,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogMatch {
    pub line: String,
    pub offset: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Execution {
    pub id: Uuid,
    pub cmd: String,
    pub status: ExecutionStatus,
}
```

**Step 2: Create log_store/mod.rs with trait**

```rust
pub mod memory;
pub mod file;

use async_trait::async_trait;
use uuid::Uuid;
use crate::types::{ExecutionStatus, LogMatch};

#[async_trait]
pub trait LogStore: Send + Sync + Clone + 'static {
    async fn create(&self, id: Uuid, cmd: String) -> Result<(), String>;
    async fn append(&self, id: Uuid, data: &[u8]) -> Result<(), String>;
    async fn read(&self, id: Uuid, offset: u64) -> Result<(Vec<u8>, u64), String>;
    async fn search(&self, id: Uuid, pattern: &str) -> Result<Vec<LogMatch>, String>;
    async fn get_status(&self, id: Uuid) -> Result<ExecutionStatus, String>;
    async fn set_status(&self, id: Uuid, status: ExecutionStatus) -> Result<(), String>;
}

#[derive(Clone)]
pub enum AnyLogStore {
    Memory(memory::InMemoryLogStore),
    File(file::FileLogStore),
}

#[async_trait]
impl LogStore for AnyLogStore {
    async fn create(&self, id: Uuid, cmd: String) -> Result<(), String> {
        match self {
            AnyLogStore::Memory(s) => s.create(id, cmd).await,
            AnyLogStore::File(s) => s.create(id, cmd).await,
        }
    }

    async fn append(&self, id: Uuid, data: &[u8]) -> Result<(), String> {
        match self {
            AnyLogStore::Memory(s) => s.append(id, data).await,
            AnyLogStore::File(s) => s.append(id, data).await,
        }
    }

    async fn read(&self, id: Uuid, offset: u64) -> Result<(Vec<u8>, u64), String> {
        match self {
            AnyLogStore::Memory(s) => s.read(id, offset).await,
            AnyLogStore::File(s) => s.read(id, offset).await,
        }
    }

    async fn search(&self, id: Uuid, pattern: &str) -> Result<Vec<LogMatch>, String> {
        match self {
            AnyLogStore::Memory(s) => s.search(id, pattern).await,
            AnyLogStore::File(s) => s.search(id, pattern).await,
        }
    }

    async fn get_status(&self, id: Uuid) -> Result<ExecutionStatus, String> {
        match self {
            AnyLogStore::Memory(s) => s.get_status(id).await,
            AnyLogStore::File(s) => s.get_status(id).await,
        }
    }

    async fn set_status(&self, id: Uuid, status: ExecutionStatus) -> Result<(), String> {
        match self {
            AnyLogStore::Memory(s) => s.set_status(id, status).await,
            AnyLogStore::File(s) => s.set_status(id, status).await,
        }
    }
}
```

**Step 3: Update main.rs to include modules**

```rust
use anyhow::Result;

mod types;
mod log_store;

#[tokio::main]
async fn main() -> Result<()> {
    println!("mcp-exec server");
    Ok(())
}
```

**Step 4: Create placeholder files for memory.rs and file.rs**

Create `src/log_store/memory.rs`:
```rust
use async_trait::async_trait;
use uuid::Uuid;
use crate::log_store::LogStore;
use crate::types::{ExecutionStatus, LogMatch};

#[derive(Clone)]
pub struct InMemoryLogStore;

#[async_trait]
impl LogStore for InMemoryLogStore {
    async fn create(&self, _id: Uuid, _cmd: String) -> Result<(), String> { todo!() }
    async fn append(&self, _id: Uuid, _data: &[u8]) -> Result<(), String> { todo!() }
    async fn read(&self, _id: Uuid, _offset: u64) -> Result<(Vec<u8>, u64), String> { todo!() }
    async fn search(&self, _id: Uuid, _pattern: &str) -> Result<Vec<LogMatch>, String> { todo!() }
    async fn get_status(&self, _id: Uuid) -> Result<ExecutionStatus, String> { todo!() }
    async fn set_status(&self, _id: Uuid, _status: ExecutionStatus) -> Result<(), String> { todo!() }
}
```

Create `src/log_store/file.rs`:
```rust
use async_trait::async_trait;
use uuid::Uuid;
use crate::log_store::LogStore;
use crate::types::{ExecutionStatus, LogMatch};

#[derive(Clone)]
pub struct FileLogStore;

#[async_trait]
impl LogStore for FileLogStore {
    async fn create(&self, _id: Uuid, _cmd: String) -> Result<(), String> { todo!() }
    async fn append(&self, _id: Uuid, _data: &[u8]) -> Result<(), String> { todo!() }
    async fn read(&self, _id: Uuid, _offset: u64) -> Result<(Vec<u8>, u64), String> { todo!() }
    async fn search(&self, _id: Uuid, _pattern: &str) -> Result<Vec<LogMatch>, String> { todo!() }
    async fn get_status(&self, _id: Uuid) -> Result<ExecutionStatus, String> { todo!() }
    async fn set_status(&self, _id: Uuid, _status: ExecutionStatus) -> Result<(), String> { todo!() }
}
```

**Step 5: Verify build**

Run: `cargo build`
Expected: Compiles successfully

**Step 6: Commit**

```bash
git add src/types.rs src/log_store/
git commit -m "feat: add LogStore trait and type definitions"
```

---

### Task 3: InMemoryLogStore Implementation

**Files:**
- Modify: `src/log_store/memory.rs`

**Step 1: Implement InMemoryLogStore**

```rust
use async_trait::async_trait;
use regex::Regex;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::log_store::LogStore;
use crate::types::{ExecutionStatus, LogMatch};

struct ExecutionData {
    cmd: String,
    status: ExecutionStatus,
    logs: Vec<u8>,
}

#[derive(Clone)]
pub struct InMemoryLogStore {
    data: Arc<RwLock<HashMap<Uuid, ExecutionData>>>,
}

impl InMemoryLogStore {
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl LogStore for InMemoryLogStore {
    async fn create(&self, id: Uuid, cmd: String) -> Result<(), String> {
        let mut data = self.data.write().await;
        if data.contains_key(&id) {
            return Err(format!("Execution {} already exists", id));
        }
        data.insert(id, ExecutionData {
            cmd,
            status: ExecutionStatus::Running,
            logs: Vec::new(),
        });
        Ok(())
    }

    async fn append(&self, id: Uuid, bytes: &[u8]) -> Result<(), String> {
        let mut data = self.data.write().await;
        let exec = data.get_mut(&id).ok_or_else(|| format!("Execution {} not found", id))?;
        exec.logs.extend_from_slice(bytes);
        Ok(())
    }

    async fn read(&self, id: Uuid, offset: u64) -> Result<(Vec<u8>, u64), String> {
        let data = self.data.read().await;
        let exec = data.get(&id).ok_or_else(|| format!("Execution {} not found", id))?;
        let offset = offset as usize;
        if offset >= exec.logs.len() {
            return Ok((Vec::new(), exec.logs.len() as u64));
        }
        let bytes = exec.logs[offset..].to_vec();
        let new_offset = exec.logs.len() as u64;
        Ok((bytes, new_offset))
    }

    async fn search(&self, id: Uuid, pattern: &str) -> Result<Vec<LogMatch>, String> {
        let data = self.data.read().await;
        let exec = data.get(&id).ok_or_else(|| format!("Execution {} not found", id))?;

        let regex = Regex::new(pattern).map_err(|e| format!("Invalid regex: {}", e))?;
        let logs_str = String::from_utf8_lossy(&exec.logs);

        let mut matches = Vec::new();
        let mut byte_offset: u64 = 0;

        for line in logs_str.lines() {
            if regex.is_match(line) {
                matches.push(LogMatch {
                    line: line.to_string(),
                    offset: byte_offset,
                });
            }
            byte_offset += line.len() as u64 + 1; // +1 for newline
        }

        Ok(matches)
    }

    async fn get_status(&self, id: Uuid) -> Result<ExecutionStatus, String> {
        let data = self.data.read().await;
        let exec = data.get(&id).ok_or_else(|| format!("Execution {} not found", id))?;
        Ok(exec.status.clone())
    }

    async fn set_status(&self, id: Uuid, status: ExecutionStatus) -> Result<(), String> {
        let mut data = self.data.write().await;
        let exec = data.get_mut(&id).ok_or_else(|| format!("Execution {} not found", id))?;
        exec.status = status;
        Ok(())
    }
}
```

**Step 2: Verify build**

Run: `cargo build`
Expected: Compiles successfully

**Step 3: Commit**

```bash
git add src/log_store/memory.rs
git commit -m "feat: implement InMemoryLogStore"
```

---

### Task 4: FileLogStore Implementation

**Files:**
- Modify: `src/log_store/file.rs`

**Step 1: Implement FileLogStore**

```rust
use async_trait::async_trait;
use regex::Regex;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use uuid::Uuid;

use crate::log_store::LogStore;
use crate::types::{ExecutionStatus, LogMatch};

#[derive(Clone)]
pub struct FileLogStore {
    dir: PathBuf,
}

impl FileLogStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        let dir = dir.into();
        std::fs::create_dir_all(&dir).ok();
        Self { dir }
    }

    fn log_path(&self, id: Uuid) -> PathBuf {
        self.dir.join(format!("{}.log", id))
    }

    fn status_path(&self, id: Uuid) -> PathBuf {
        self.dir.join(format!("{}.status", id))
    }

    fn meta_path(&self, id: Uuid) -> PathBuf {
        self.dir.join(format!("{}.meta", id))
    }
}

#[async_trait]
impl LogStore for FileLogStore {
    async fn create(&self, id: Uuid, cmd: String) -> Result<(), String> {
        let log_path = self.log_path(id);
        let status_path = self.status_path(id);
        let meta_path = self.meta_path(id);

        if log_path.exists() {
            return Err(format!("Execution {} already exists", id));
        }

        std::fs::File::create(&log_path).map_err(|e| e.to_string())?;
        std::fs::write(&meta_path, &cmd).map_err(|e| e.to_string())?;

        let status = serde_json::to_string(&ExecutionStatus::Running)
            .map_err(|e| e.to_string())?;
        std::fs::write(&status_path, status).map_err(|e| e.to_string())?;

        Ok(())
    }

    async fn append(&self, id: Uuid, data: &[u8]) -> Result<(), String> {
        let path = self.log_path(id);
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .map_err(|e| format!("Failed to open log file: {}", e))?;
        file.write_all(data).map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn read(&self, id: Uuid, offset: u64) -> Result<(Vec<u8>, u64), String> {
        let path = self.log_path(id);
        let mut file = std::fs::File::open(&path)
            .map_err(|e| format!("Failed to open log file: {}", e))?;

        let file_len = file.metadata().map_err(|e| e.to_string())?.len();

        if offset >= file_len {
            return Ok((Vec::new(), file_len));
        }

        file.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;

        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer).map_err(|e| e.to_string())?;

        Ok((buffer, file_len))
    }

    async fn search(&self, id: Uuid, pattern: &str) -> Result<Vec<LogMatch>, String> {
        let path = self.log_path(id);
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read log file: {}", e))?;

        let regex = Regex::new(pattern).map_err(|e| format!("Invalid regex: {}", e))?;

        let mut matches = Vec::new();
        let mut byte_offset: u64 = 0;

        for line in content.lines() {
            if regex.is_match(line) {
                matches.push(LogMatch {
                    line: line.to_string(),
                    offset: byte_offset,
                });
            }
            byte_offset += line.len() as u64 + 1;
        }

        Ok(matches)
    }

    async fn get_status(&self, id: Uuid) -> Result<ExecutionStatus, String> {
        let path = self.status_path(id);
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read status file: {}", e))?;
        serde_json::from_str(&content).map_err(|e| e.to_string())
    }

    async fn set_status(&self, id: Uuid, status: ExecutionStatus) -> Result<(), String> {
        let path = self.status_path(id);
        let content = serde_json::to_string(&status).map_err(|e| e.to_string())?;
        std::fs::write(&path, content).map_err(|e| e.to_string())?;
        Ok(())
    }
}
```

**Step 2: Verify build**

Run: `cargo build`
Expected: Compiles successfully

**Step 3: Commit**

```bash
git add src/log_store/file.rs
git commit -m "feat: implement FileLogStore"
```

---

### Task 5: Executor Module

**Files:**
- Create: `src/executor.rs`
- Modify: `src/main.rs`

**Step 1: Create executor.rs**

```rust
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::time::{timeout, Duration};
use uuid::Uuid;

use crate::log_store::LogStore;
use crate::types::ExecutionStatus;

pub async fn spawn_command<S: LogStore>(
    store: S,
    id: Uuid,
    cmd: String,
    timeout_secs: u64,
) {
    tokio::spawn(async move {
        let result = run_command(&store, id, &cmd, timeout_secs).await;

        let status = match result {
            Ok(exit_code) => ExecutionStatus::Completed(exit_code),
            Err(e) if e.contains("timed out") => ExecutionStatus::Timeout,
            Err(e) => ExecutionStatus::Failed(e),
        };

        if let Err(e) = store.set_status(id, status).await {
            tracing::error!("Failed to set status for {}: {}", id, e);
        }
    });
}

async fn run_command<S: LogStore>(
    store: &S,
    id: Uuid,
    cmd: &str,
    timeout_secs: u64,
) -> Result<i32, String> {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn process: {}", e))?;

    let stdout = child.stdout.take().ok_or("Failed to capture stdout")?;
    let stderr = child.stderr.take().ok_or("Failed to capture stderr")?;

    let store_stdout = store.clone();
    let store_stderr = store.clone();

    // Stream stdout
    let stdout_handle = tokio::spawn(async move {
        let mut reader = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            let line_with_newline = format!("{}\n", line);
            if let Err(e) = store_stdout.append(id, line_with_newline.as_bytes()).await {
                tracing::error!("Failed to append stdout: {}", e);
            }
        }
    });

    // Stream stderr
    let stderr_handle = tokio::spawn(async move {
        let mut reader = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            let line_with_newline = format!("{}\n", line);
            if let Err(e) = store_stderr.append(id, line_with_newline.as_bytes()).await {
                tracing::error!("Failed to append stderr: {}", e);
            }
        }
    });

    // Wait for process with timeout
    let wait_result = timeout(Duration::from_secs(timeout_secs), child.wait()).await;

    match wait_result {
        Ok(Ok(exit_status)) => {
            // Wait for output streams to finish
            let _ = stdout_handle.await;
            let _ = stderr_handle.await;
            Ok(exit_status.code().unwrap_or(-1))
        }
        Ok(Err(e)) => Err(format!("Process error: {}", e)),
        Err(_) => {
            // Timeout - kill the process
            child.kill().await.ok();
            Err("Process timed out".to_string())
        }
    }
}
```

**Step 2: Update main.rs**

```rust
use anyhow::Result;

mod types;
mod log_store;
mod executor;

#[tokio::main]
async fn main() -> Result<()> {
    println!("mcp-exec server");
    Ok(())
}
```

**Step 3: Verify build**

Run: `cargo build`
Expected: Compiles successfully

**Step 4: Commit**

```bash
git add src/executor.rs src/main.rs
git commit -m "feat: add executor module for async command execution"
```

---

### Task 6: MCP Service with Tools

**Files:**
- Create: `src/service.rs`
- Modify: `src/main.rs`

**Step 1: Create service.rs with ExecService and tools**

```rust
use rmcp::{
    model::{ServerCapabilities, ServerInfo},
    Error as McpError, RoleServer, ServerHandler,
    model::*,
    service::RequestContext,
    tool,
};
use serde_json::json;
use uuid::Uuid;

use crate::executor::spawn_command;
use crate::log_store::{AnyLogStore, LogStore};
use crate::types::{ExecutionStatus, LogMatch};

#[derive(Clone)]
pub struct ExecService {
    store: AnyLogStore,
}

// Response types
#[derive(Debug, Clone)]
pub struct ExecResponse {
    pub id: String,
    pub status: String,
}

impl IntoContents for ExecResponse {
    fn into_contents(self) -> Vec<Content> {
        match Content::json(json!({
            "id": self.id,
            "status": self.status,
        })) {
            Ok(content) => vec![content],
            Err(e) => vec![Content::text(format!("Error: {}", e))],
        }
    }
}

#[derive(Debug, Clone)]
pub struct StreamLogsResponse {
    pub logs: String,
    pub next_offset: u64,
    pub status: String,
}

impl IntoContents for StreamLogsResponse {
    fn into_contents(self) -> Vec<Content> {
        match Content::json(json!({
            "logs": self.logs,
            "next_offset": self.next_offset,
            "status": self.status,
        })) {
            Ok(content) => vec![content],
            Err(e) => vec![Content::text(format!("Error: {}", e))],
        }
    }
}

#[derive(Debug, Clone)]
pub struct SearchLogsResponse {
    pub matches: Vec<LogMatch>,
}

impl IntoContents for SearchLogsResponse {
    fn into_contents(self) -> Vec<Content> {
        match Content::json(json!({
            "matches": self.matches,
        })) {
            Ok(content) => vec![content],
            Err(e) => vec![Content::text(format!("Error: {}", e))],
        }
    }
}

fn status_to_string(status: &ExecutionStatus) -> String {
    match status {
        ExecutionStatus::Running => "running".to_string(),
        ExecutionStatus::Completed(code) => format!("completed:{}", code),
        ExecutionStatus::Failed(msg) => format!("failed:{}", msg),
        ExecutionStatus::Timeout => "timeout".to_string(),
    }
}

#[tool(tool_box)]
impl ExecService {
    pub fn new(store: AnyLogStore) -> Self {
        Self { store }
    }

    /// Execute a shell command asynchronously. Returns immediately with a UUID to track the execution.
    #[tool(description = "Execute a shell command asynchronously. Returns a UUID to track execution. Use stream_logs to get output.")]
    pub async fn exec(
        &self,
        #[tool(param, description = "The shell command to execute")] cmd: String,
        #[tool(param, description = "Timeout in seconds")] timeout: u64,
    ) -> ExecResponse {
        let id = Uuid::new_v4();

        if let Err(e) = self.store.create(id, cmd.clone()).await {
            return ExecResponse {
                id: id.to_string(),
                status: format!("error: {}", e),
            };
        }

        spawn_command(self.store.clone(), id, cmd, timeout).await;

        ExecResponse {
            id: id.to_string(),
            status: "started".to_string(),
        }
    }

    /// Stream logs from an execution starting at the given byte offset.
    #[tool(description = "Stream logs from an execution. Returns logs from offset, next_offset for pagination, and current status.")]
    pub async fn stream_logs(
        &self,
        #[tool(param, description = "The execution UUID")] id: String,
        #[tool(param, description = "Byte offset to start reading from")] offset: u64,
    ) -> StreamLogsResponse {
        let uuid = match Uuid::parse_str(&id) {
            Ok(u) => u,
            Err(e) => return StreamLogsResponse {
                logs: format!("Invalid UUID: {}", e),
                next_offset: 0,
                status: "error".to_string(),
            },
        };

        let (bytes, next_offset) = match self.store.read(uuid, offset).await {
            Ok(r) => r,
            Err(e) => return StreamLogsResponse {
                logs: format!("Error reading logs: {}", e),
                next_offset: offset,
                status: "error".to_string(),
            },
        };

        let status = match self.store.get_status(uuid).await {
            Ok(s) => status_to_string(&s),
            Err(e) => format!("error: {}", e),
        };

        StreamLogsResponse {
            logs: String::from_utf8_lossy(&bytes).to_string(),
            next_offset,
            status,
        }
    }

    /// Search logs for a regex pattern. Returns matching lines with their byte offsets.
    #[tool(description = "Search logs for a regex pattern. Returns matching lines with byte offsets.")]
    pub async fn search_logs(
        &self,
        #[tool(param, description = "The execution UUID")] id: String,
        #[tool(param, description = "Regex pattern to search for")] pattern: String,
    ) -> SearchLogsResponse {
        let uuid = match Uuid::parse_str(&id) {
            Ok(u) => u,
            Err(_) => return SearchLogsResponse { matches: vec![] },
        };

        match self.store.search(uuid, &pattern).await {
            Ok(matches) => SearchLogsResponse { matches },
            Err(_) => SearchLogsResponse { matches: vec![] },
        }
    }
}

#[tool(tool_box)]
impl ServerHandler for ExecService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some("Shell command execution service with log streaming and search".into()),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }

    async fn initialize(
        &self,
        _request: InitializeRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, McpError> {
        Ok(self.get_info())
    }
}
```

**Step 2: Update main.rs**

```rust
use anyhow::Result;

mod types;
mod log_store;
mod executor;
mod service;

#[tokio::main]
async fn main() -> Result<()> {
    println!("mcp-exec server");
    Ok(())
}
```

**Step 3: Verify build**

Run: `cargo build`
Expected: Compiles successfully

**Step 4: Commit**

```bash
git add src/service.rs src/main.rs
git commit -m "feat: add ExecService with exec, stream_logs, search_logs tools"
```

---

### Task 7: CLI and Transport Setup

**Files:**
- Modify: `src/main.rs`

**Step 1: Implement full main.rs with CLI and transports**

```rust
use anyhow::Result;
use clap::Parser;
use hyper::{
    body::Incoming,
    header::{HeaderValue, UPGRADE},
    Request, StatusCode,
};
use hyper_util::rt::TokioIo;
use rmcp::transport::sse_server::{SseServer, SseServerConfig};
use rmcp::{transport::stdio, ServiceExt};
use tokio_util::sync::CancellationToken;

mod executor;
mod log_store;
mod service;
mod types;

use log_store::{file::FileLogStore, memory::InMemoryLogStore, AnyLogStore};
use service::ExecService;

#[derive(Parser, Debug)]
#[command(author, version, about = "MCP server for shell command execution")]
struct Cli {
    /// Directory path for log storage. If not specified, uses in-memory storage.
    #[arg(long)]
    directory_path: Option<String>,

    /// HTTP port to listen on (if not specified, uses stdio transport)
    #[arg(long, conflicts_with = "sse_port")]
    http_port: Option<u16>,

    /// SSE port to listen on (if not specified, uses stdio transport)
    #[arg(long, conflicts_with = "http_port")]
    sse_port: Option<u16>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let cli = Cli::parse();
    tracing::info!(?cli, "Starting MCP exec server");

    let store = match cli.directory_path {
        Some(dir) => {
            tracing::info!("Using file-based log storage at {}", dir);
            AnyLogStore::File(FileLogStore::new(dir))
        }
        None => {
            tracing::info!("Using in-memory log storage");
            AnyLogStore::Memory(InMemoryLogStore::new())
        }
    };

    if let Some(port) = cli.http_port {
        tracing::info!("Starting HTTP transport on port {}", port);
        start_http_server(store, port).await?;
    } else if let Some(port) = cli.sse_port {
        tracing::info!("Starting SSE transport on port {}", port);
        start_sse_server(store, port).await?;
    } else {
        tracing::info!("Starting stdio transport");
        let service = ExecService::new(store).serve(stdio()).await?;
        service.waiting().await?;
    }

    Ok(())
}

async fn http_handler(
    req: Request<Incoming>,
    store: AnyLogStore,
) -> Result<hyper::Response<String>, hyper::Error> {
    tokio::spawn(async move {
        let upgraded = hyper::upgrade::on(req).await?;
        let service = ExecService::new(store)
            .serve(TokioIo::new(upgraded))
            .await?;
        service.waiting().await?;
        anyhow::Result::<()>::Ok(())
    });

    let mut response = hyper::Response::new(String::new());
    *response.status_mut() = StatusCode::SWITCHING_PROTOCOLS;
    response
        .headers_mut()
        .insert(UPGRADE, HeaderValue::from_static("mcp"));
    Ok(response)
}

async fn start_http_server(store: AnyLogStore, port: u16) -> Result<()> {
    let addr = format!("0.0.0.0:{}", port);
    let tcp_listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("HTTP server listening on {}", addr);

    loop {
        let (stream, addr) = tcp_listener.accept().await?;
        tracing::info!("Accepted connection from: {}", addr);
        let store_clone = store.clone();

        let service =
            hyper::service::service_fn(move |req| http_handler(req, store_clone.clone()));

        let conn = hyper::server::conn::http1::Builder::new()
            .serve_connection(TokioIo::new(stream), service)
            .with_upgrades();

        tokio::spawn(async move {
            if let Err(err) = conn.await {
                tracing::error!("Connection error: {:?}", err);
            }
        });
    }
}

async fn start_sse_server(store: AnyLogStore, port: u16) -> Result<()> {
    let addr = format!("0.0.0.0:{}", port).parse()?;

    let config = SseServerConfig {
        bind: addr,
        sse_path: "/sse".to_string(),
        post_path: "/message".to_string(),
        ct: CancellationToken::new(),
        sse_keep_alive: Some(std::time::Duration::from_secs(15)),
    };

    let (sse_server, router) = SseServer::new(config);

    let listener = tokio::net::TcpListener::bind(sse_server.config.bind).await?;
    tracing::info!("SSE server listening on {}", sse_server.config.bind);

    let ct = sse_server.config.ct.clone();
    let ct_shutdown = ct.child_token();

    let server = axum::serve(listener, router).with_graceful_shutdown(async move {
        ct_shutdown.cancelled().await;
        tracing::info!("SSE server shutting down");
    });

    sse_server.with_service(move || ExecService::new(store.clone()));

    tokio::spawn(async move {
        if let Err(e) = server.await {
            tracing::error!("SSE server error: {:?}", e);
        }
    });

    tokio::signal::ctrl_c().await?;
    tracing::info!("Received Ctrl+C, shutting down");
    ct.cancel();

    Ok(())
}
```

**Step 2: Verify build**

Run: `cargo build`
Expected: Compiles successfully

**Step 3: Test stdio transport**

Run: `echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}' | cargo run`
Expected: JSON response with server info

**Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat: add CLI with stdio, HTTP, and SSE transport support"
```

---

### Task 8: Integration Test

**Files:**
- Create: `tests/integration.rs`

**Step 1: Create basic integration test**

```rust
use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader, Write};

#[test]
fn test_stdio_initialize() {
    let mut child = Command::new("cargo")
        .args(["run", "--"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to start server");

    let mut stdin = child.stdin.take().expect("Failed to get stdin");
    let stdout = child.stdout.take().expect("Failed to get stdout");

    // Send initialize request
    let init_request = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}"#;
    writeln!(stdin, "{}", init_request).expect("Failed to write");
    stdin.flush().expect("Failed to flush");

    // Read response
    let reader = BufReader::new(stdout);
    let response = reader.lines().next().expect("No response").expect("Failed to read");

    assert!(response.contains("tools"));

    child.kill().ok();
}
```

**Step 2: Run test**

Run: `cargo test`
Expected: Test passes

**Step 3: Commit**

```bash
git add tests/integration.rs
git commit -m "test: add basic stdio integration test"
```

---

### Task 9: Final Verification

**Step 1: Full build**

Run: `cargo build --release`
Expected: Compiles successfully

**Step 2: Test with MCP inspector (optional)**

Run: `npx @modelcontextprotocol/inspector cargo run`
Expected: Inspector connects and shows 3 tools: exec, stream_logs, search_logs

**Step 3: Test exec tool manually**

Start server in one terminal: `cargo run`

In another terminal, send tool call via JSON-RPC (or use inspector)

**Step 4: Final commit with any fixes**

```bash
git add -A
git commit -m "chore: final cleanup and verification"
```

---

## Summary

9 tasks total:
1. Project setup
2. Log store trait and types
3. InMemoryLogStore implementation
4. FileLogStore implementation
5. Executor module
6. MCP service with tools
7. CLI and transport setup
8. Integration test
9. Final verification
