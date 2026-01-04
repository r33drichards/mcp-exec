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
- `service.rs` - MCP tool router with 3 tools: exec, stream_logs, search_logs
- `executor.rs` - Async command spawning with timeout and stdout/stderr capture
- `log_store/` - Trait-based storage abstraction with memory and file backends
- `types.rs` - ExecutionStatus, LogMatch, Execution domain types

**Transport modes:**
- Stdio (default): Uses rmcp stdio transport for local MCP integration
- HTTP: Streamable HTTP on configurable port, binds to 127.0.0.1 only

**Data flow:** Client calls exec → UUID created → async task spawns shell command → output streams to LogStore → client can stream_logs from any offset or search_logs with regex

## Key Patterns

- Trait-based abstraction: LogStore trait with AnyLogStore enum for runtime backend selection
- Fully async with Tokio multi-threaded runtime
- UUID v4 for command tracking across distributed systems
- Streaming output: logs captured as executed, not buffered
