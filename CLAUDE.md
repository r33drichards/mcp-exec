# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

mcp-exec is a Rust-based MCP (Model Context Protocol) server for asynchronous shell command execution with log streaming and searching capabilities. It allows AI agents to execute shell commands asynchronously and retrieve output with pagination and regex-based log searching.

## Build and Development Commands

```bash
# Build
cargo build --release

# Run (stdio transport, in-memory storage)
cargo run

# Run with file-based storage
cargo run -- --directory-path /tmp/mcp-exec-logs

# Run with HTTP transport
cargo run -- --directory-path /tmp/mcp-exec-logs --http-port 8080

# Run all tests
cargo test

# Run integration tests only
cargo test --test integration

# Build with Nix
nix build
```

## Architecture

```
ExecService (service.rs)
├── exec() → spawn_command (executor.rs) → LogStore
├── stream_logs() → LogStore
└── search_logs() → LogStore

LogStore Trait (log_store/mod.rs)
├── InMemoryLogStore (log_store/memory.rs) - HashMap + RwLock
└── FileLogStore (log_store/file.rs) - {id}.log, {id}.status, {id}.meta files
```

**Key modules:**
- `main.rs` - CLI args, transport selection (stdio/HTTP), server bootstrap
- `service.rs` - MCP tool router with 3 tools (exec, stream_logs, search_logs) plus the MCP Tasks (SEP-1686) handlers (enqueue_task, get_task_info, get_task_result, cancel_task, list_tasks)
- `executor.rs` - Async command spawning with timeout, cancellation, and stdout/stderr capture; owns the `TaskRegistry` of cancellation tokens
- `log_store/` - Trait-based storage abstraction with memory and file backends
- `types.rs` - ExecutionStatus, LogMatch, Execution domain types

**MCP Tasks:** The `exec` tool declares `execution.taskSupport: "optional"`, and the server advertises the `tasks` capability (`TasksCapability::server_default()`). A task-augmented `tools/call` maps onto a running command — the task id is the execution UUID, status is derived from `ExecutionStatus`, and `tasks/result` blocks until the command terminates. The handlers are hand-written (not the `#[task_handler]` macro) so the task reflects the command's lifecycle rather than `exec`'s immediate return. `rmcp` is pinned to a released crates.io version (`1.7`), not a git branch, for reproducible builds.

**Transport modes:**
- Stdio (default): Uses rmcp stdio transport for local MCP integration
- HTTP: Streamable HTTP on configurable port, binds to 127.0.0.1 only

**Data flow:** Client calls exec → UUID created → async task spawns shell command → output streams to LogStore → client can stream_logs from any offset or search_logs with regex

## Key Patterns

- Trait-based abstraction: LogStore trait with AnyLogStore enum for runtime backend selection
- Fully async with Tokio multi-threaded runtime
- UUID v4 for command tracking across distributed systems
- Streaming output: logs captured as executed, not buffered
