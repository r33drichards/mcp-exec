# MCP Exec Server Design

An MCP server that executes shell commands asynchronously and provides log streaming/search capabilities.

## Tools

| Tool | Params | Returns |
|------|--------|---------|
| `exec` | `cmd: String`, `timeout: u64` (seconds) | `{ id: UUID, status: "started" }` |
| `stream_logs` | `id: UUID`, `offset: u64` (bytes) | `{ logs: String, next_offset: u64, status: "running"\|"completed"\|"failed"\|"timeout" }` |
| `search_logs` | `id: UUID`, `pattern: String` | `{ matches: [{ line: String, offset: u64 }] }` |

## Execution Model

- **Async execution**: `exec` returns immediately with UUID, command runs in background
- **Offset-based streaming**: Agent polls `stream_logs` with offset to get incremental output
- **Regex search**: `search_logs` returns matching lines with byte offsets

## Data Structures

```rust
struct Execution {
    id: Uuid,
    cmd: String,
    status: ExecutionStatus,  // Running, Completed(i32), Failed(String), Timeout
    started_at: Instant,
}

enum ExecutionStatus {
    Running,
    Completed(i32),  // exit code
    Failed(String),  // error message
    Timeout,
}

trait LogStore: Send + Sync + Clone {
    async fn create(&self, id: Uuid) -> Result<()>;
    async fn append(&self, id: Uuid, data: &[u8]) -> Result<()>;
    async fn read(&self, id: Uuid, offset: u64) -> Result<(Vec<u8>, u64)>;
    async fn search(&self, id: Uuid, pattern: &str) -> Result<Vec<Match>>;
    async fn get_status(&self, id: Uuid) -> Result<ExecutionStatus>;
    async fn set_status(&self, id: Uuid, status: ExecutionStatus) -> Result<()>;
}

struct Match {
    line: String,
    offset: u64,
}
```

## Storage Implementations

1. **InMemoryLogStore**: `HashMap<Uuid, Execution>` behind `Arc<RwLock>`. Fast, ephemeral.
2. **FileLogStore**: Writes to `{dir}/{uuid}.log` and `{dir}/{uuid}.status`. Survives restarts.

## Execution Flow

1. `exec(cmd, timeout)` called
2. Generate UUID, create log entry (status: Running)
3. Spawn tokio task:
   - Spawn child process: `sh -c "{cmd}"`
   - Stream stdout+stderr merged into log store
   - Race against `tokio::time::timeout`
   - On completion: update status (Completed/Failed/Timeout)
   - Kill child on timeout
4. Return `{ id, status: "started" }` immediately

## CLI Interface

```bash
# In-memory storage (default)
mcp-exec

# File-based storage
mcp-exec --directory-path /var/log/mcp-exec

# With transports
mcp-exec --sse-port 8080
mcp-exec --http-port 8080
mcp-exec --directory-path /tmp/logs --sse-port 8080
```

## Project Structure

```
mcp-exec/
├── Cargo.toml
├── src/
│   ├── main.rs           # CLI parsing, transport setup
│   ├── service.rs        # ExecService with tool implementations
│   ├── log_store/
│   │   ├── mod.rs        # LogStore trait + AnyLogStore enum
│   │   ├── memory.rs     # InMemoryLogStore
│   │   └── file.rs       # FileLogStore
│   └── executor.rs       # Process spawning, timeout handling
```

## Dependencies

- `rmcp` - MCP protocol implementation
- `tokio` - Async runtime, process spawning, timeouts
- `clap` - CLI argument parsing
- `uuid` - UUID generation
- `regex` - Log search
- `serde`, `serde_json` - Serialization
- `tracing`, `tracing-subscriber` - Logging
