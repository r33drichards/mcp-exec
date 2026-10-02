# mcp-exec

A Rust-based [MCP (Model Context Protocol)](https://modelcontextprotocol.io/) server for asynchronous shell command execution with log streaming and searching capabilities.

## Features

- **Async command execution** - Commands run in the background, returning immediately with a UUID for tracking
- **Log streaming** - Retrieve output with byte offset pagination for efficient large log handling
- **Log searching** - Regex-based search across command output
- **MCP Tasks support** - Implements the [Tasks extension (SEP-1686)](https://modelcontextprotocol.io/specification/2025-11-25/basic/utilities/tasks); task-capable clients can run `exec` as a task and poll/await/cancel it via the standard `tasks/*` methods
- **Dual transport** - Supports both stdio and streamable HTTP transports
- **Flexible storage** - In-memory or file-based log persistence
- **NixOS module** - First-class NixOS/systemd integration

## MCP Tools

| Tool | Description |
|------|-------------|
| `exec` | Execute a shell command asynchronously. Returns a UUID to track execution. Task-capable (`execution.taskSupport: "optional"`). |
| `stream_logs` | Stream logs from an execution with byte offset pagination. |
| `search_logs` | Search logs using regex patterns. Returns matching lines with offsets. |

## MCP Tasks

The server advertises the [`tasks` capability](https://modelcontextprotocol.io/specification/2025-11-25/basic/utilities/tasks) (`tasks.list`, `tasks.cancel`, and `tasks.requests.tools.call`). Clients that support tasks may augment a `tools/call` to `exec` with a `task` object, in which case the task tracks the command's full lifecycle rather than returning immediately:

| Method | Behavior |
|--------|----------|
| `tools/call` (with `task`) | Spawns the command and returns a `CreateTaskResult`. The task id is the execution UUID, so it also works with `stream_logs`/`search_logs`. |
| `tasks/get` | Returns the task's current status (`working` → `completed`/`failed`/`cancelled`). |
| `tasks/result` | Blocks until the command finishes, then returns its full output as a `CallToolResult`. |
| `tasks/cancel` | Kills the running command and marks the task `cancelled`. |
| `tasks/list` | Lists all known executions as tasks. |

Status mapping: a finished command is `completed` (its exit code is included in the result text), while timeouts and spawn failures map to `failed`. Clients that do not support tasks can keep using the original `exec` → `stream_logs`/`search_logs` flow unchanged.

> **Note on task isolation:** task ids are random UUIDs and the server does not bind them to an authorization context, so any requestor that knows a task id can read or cancel it. This matches the single-user/local design of the server; do not expose it untrusted on a shared network.

## Installation

### Using Cargo

```bash
cargo install --path .
```

### Using Nix

```bash
nix build
./result/bin/mcp-exec --help
```

### Using Docker

Pre-built Docker images are published to the [GitHub Container Registry](https://github.com/r33drichards/mcp-exec/pkgs/container/mcp-exec) at `ghcr.io/r33drichards/mcp-exec` for both amd64 and arm64 architectures.

**Pull the image:**

```bash
docker pull ghcr.io/r33drichards/mcp-exec:latest
```

**Run with HTTP transport (recommended for Docker):**

```bash
# Basic run with HTTP on port 8080
docker run -d \
  --name mcp-exec \
  -p 8080:8080 \
  -v mcp-exec-data:/data \
  ghcr.io/r33drichards/mcp-exec:latest \
  --http-port 8080 \
  --bind-address 0.0.0.0 \
  --directory-path /data

# Connect to http://localhost:8080/mcp
```

**Run with custom port and volume:**

```bash
docker run -d \
  --name mcp-exec \
  -p 19222:19222 \
  -v /path/to/logs:/var/lib/mcp-exec \
  ghcr.io/r33drichards/mcp-exec:latest \
  --http-port 19222 \
  --bind-address 0.0.0.0 \
  --directory-path /var/lib/mcp-exec
```

**Using Docker Compose:**

Create a `docker-compose.yml` file:

```yaml
services:
  mcp-exec:
    image: ghcr.io/r33drichards/mcp-exec:latest
    container_name: mcp-exec
    ports:
      - "8080:8080"
    volumes:
      - mcp-exec-data:/data
    command: >
      --http-port 8080
      --bind-address 0.0.0.0
      --directory-path /data
    restart: unless-stopped

volumes:
  mcp-exec-data:
```

Then start the service:

```bash
# Docker Compose V2 (recommended)
docker compose up -d

# Docker Compose V1 (legacy)
docker-compose up -d

# View logs
docker compose logs -f

# Stop service
docker compose down
```

**Notes:**
- Docker deployments must use HTTP transport (stdio transport is not compatible with Docker)
- Use `--bind-address 0.0.0.0` to allow connections from outside the container
- Use volume mounts to persist command logs across container restarts
- The MCP endpoint is available at `http://localhost:<port>/mcp`

## Usage

### Stdio Transport (default)

For local MCP integration with tools like Claude Desktop:

```bash
# In-memory storage
mcp-exec

# File-based storage
mcp-exec --directory-path /tmp/mcp-exec-logs
```

### HTTP Transport

For remote/network access:

```bash
mcp-exec --http-port 8080 --directory-path /var/lib/mcp-exec

# Bind to all interfaces (for remote access)
mcp-exec --http-port 8080 --bind-address 0.0.0.0 --directory-path /var/lib/mcp-exec
```

### Running next to a web browser

The HTTP transport has no authentication. On the default loopback bind, the
server only answers requests whose `Host` is a loopback name (protection against
DNS rebinding) and requires `Content-Type: application/json`, which makes a
browser send a CORS preflight that the server does not answer. If a web browser
runs on the same machine, or in the same network namespace (a container next to
it in a pod), add `--reject-browser-requests`: any request carrying an `Origin`
or `Sec-Fetch-*` header is then answered `403`, so a page open in that browser
cannot call the server whatever the browser's CORS settings. Browsers set those
headers themselves and page script cannot remove them; MCP clients send neither.

```bash
mcp-exec --http-port 8080 --reject-browser-requests
```

Do not use the flag with a client that itself runs in a browser (for example a
web-based MCP inspector).

## Client Integration

### Claude for Desktop

1. Install mcp-exec using Cargo or Nix (see [Installation](#installation)).
2. Open Claude Desktop → Settings → Developer → Edit Config.
3. Add the server to `claude_desktop_config.json`:

**With file-based storage (recommended):**
```json
{
  "mcpServers": {
    "exec": {
      "command": "mcp-exec",
      "args": ["--directory-path", "/tmp/mcp-exec-logs"]
    }
  }
}
```

**With in-memory storage:**
```json
{
  "mcpServers": {
    "exec": {
      "command": "mcp-exec"
    }
  }
}
```

4. Restart Claude Desktop. The exec tools will appear under the hammer icon.

### Cursor

1. Install mcp-exec using Cargo or Nix.
2. Create or edit `.cursor/mcp.json` in your project root:

```json
{
  "mcpServers": {
    "exec": {
      "command": "mcp-exec",
      "args": ["--directory-path", "/tmp/mcp-exec-logs"]
    }
  }
}
```

3. Restart Cursor. The MCP tools will be available in the UI.

### Claude Code CLI

**For local stdio transport:**
```bash
claude mcp add exec -- mcp-exec --directory-path /tmp/mcp-exec-logs
```

**For remote HTTP server:**
```bash
# If mcp-exec is running on a remote server with HTTP transport
claude mcp add exec -t http http://your-server:8080/mcp
```

Then test by running `claude` and asking: "Execute `echo hello world`"

### Windsurf

1. Install mcp-exec using Cargo or Nix.
2. Open Windsurf → Settings → MCP Servers.
3. Add a new server configuration:

```json
{
  "mcpServers": {
    "exec": {
      "command": "mcp-exec",
      "args": ["--directory-path", "/tmp/mcp-exec-logs"]
    }
  }
}
```

4. Restart Windsurf to apply changes.

### Zed

1. Install mcp-exec using Cargo or Nix.
2. Edit your Zed settings (`~/.config/zed/settings.json`):

```json
{
  "context_servers": {
    "exec": {
      "command": {
        "path": "mcp-exec",
        "args": ["--directory-path", "/tmp/mcp-exec-logs"]
      }
    }
  }
}
```

3. Restart Zed to enable the MCP server.

### Generic MCP Client (HTTP)

For any MCP client that supports HTTP transport, start the server and connect:

```bash
# Start the server
mcp-exec --http-port 8080 --directory-path /var/lib/mcp-exec

# Connect using the streamable HTTP endpoint
# URL: http://localhost:8080/mcp
```

## NixOS Module

The flake provides a NixOS module for running mcp-exec as a system service.

### Basic Setup

Add the flake to your NixOS configuration:

```nix
# flake.nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    mcp-exec.url = "github:r33drichards/mcp-exec";
  };

  outputs = { self, nixpkgs, mcp-exec, ... }: {
    nixosConfigurations.myhost = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        mcp-exec.nixosModules.default
        ./configuration.nix
      ];
    };
  };
}
```

### Configuration Examples

**Minimal configuration:**

```nix
{
  services.mcp-exec = {
    enable = true;
  };
}
```

This starts mcp-exec on port 19222, bound to all interfaces, with file-based storage at `/var/lib/mcp-exec`.

**Full configuration with all options:**

```nix
{
  services.mcp-exec = {
    enable = true;

    # Network settings
    port = 19222;                    # HTTP port (default: 19222)
    bindAddress = "0.0.0.0";         # Bind address (default: "0.0.0.0")
    openFirewall = true;             # Open port in firewall (default: false)

    # Storage
    directoryPath = "/var/lib/mcp-exec";  # Log storage directory

    # Service user (auto-created if using defaults)
    user = "mcp-exec";
    group = "mcp-exec";

    # Sudo access for privileged commands
    sudo = {
      enable = true;                 # Grant passwordless sudo (default: false)
      commands = [ "ALL" ];          # Allowed commands (default: ["ALL"])
    };

    # Additional CLI arguments
    extraArgs = [ ];
  };
}
```

**Localhost-only (secure):**

```nix
{
  services.mcp-exec = {
    enable = true;
    bindAddress = "127.0.0.1";  # Only accept local connections
  };
}
```

**With restricted sudo access:**

```nix
{
  services.mcp-exec = {
    enable = true;
    sudo = {
      enable = true;
      commands = [
        "/run/current-system/sw/bin/systemctl status *"
        "/run/current-system/sw/bin/journalctl *"
      ];
    };
  };
}
```

### Module Options Reference

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `enable` | bool | `false` | Enable the mcp-exec service |
| `package` | package | `(flake)` | The mcp-exec package to use |
| `port` | port | `19222` | HTTP port to listen on |
| `bindAddress` | string | `"0.0.0.0"` | Address to bind the HTTP server to |
| `directoryPath` | path | `"/var/lib/mcp-exec"` | Directory for storing command logs |
| `user` | string | `"mcp-exec"` | User account for the service |
| `group` | string | `"mcp-exec"` | Group for the service |
| `openFirewall` | bool | `false` | Open the firewall port |
| `sudo.enable` | bool | `false` | Grant passwordless sudo access |
| `sudo.commands` | list of string | `["ALL"]` | Allowed sudo commands |
| `extraArgs` | list of string | `[]` | Additional CLI arguments |

### Service Management

```bash
# Check service status
systemctl status mcp-exec

# View logs
journalctl -u mcp-exec -f

# Restart service
systemctl restart mcp-exec
```

## Development

```bash
# Build
cargo build --release

# Run tests
cargo test

# Run integration tests
cargo test --test integration

# Run NixOS VM tests
nix build .#checks.x86_64-linux.vm-basic
nix build .#checks.x86_64-linux.vm-full
nix build .#checks.x86_64-linux.vm-multi-machine
```

## Security Considerations

- By default, the service binds to all interfaces (`0.0.0.0`). For local-only access, set `bindAddress = "127.0.0.1"`.
- The service executes arbitrary shell commands. Only expose it to trusted networks/clients.
- When `sudo.enable = true`, the service user gains passwordless sudo access. Restrict `sudo.commands` in production.
- Consider using a reverse proxy with authentication for remote access.

## License

MIT
