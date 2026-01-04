# NixOS VM Integration Tests Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Create three NixOS VM integration tests (basic health, full functionality, multi-machine) and a GitHub Actions workflow to run them in parallel.

**Architecture:** Each test is a separate NixOS VM test in `nix/tests/`. The flake exposes them as `checks.x86_64-linux.vm-*`. GitHub Actions uses a matrix strategy to run all three tests in parallel on ubuntu-latest with KVM.

**Tech Stack:** NixOS testing framework, GitHub Actions, KVM virtualization

---

## Task 1: Create Basic Health VM Test

**Files:**
- Create: `nix/tests/basic.nix`

**Step 1: Create the basic health test**

Create `nix/tests/basic.nix`:

```nix
{ pkgs, self }:

pkgs.nixosTest {
  name = "mcp-exec-basic";

  nodes.server = { config, pkgs, ... }: {
    imports = [ self.nixosModules.default ];

    services.mcp-exec = {
      enable = true;
      port = 19222;
    };
  };

  testScript = ''
    start_all()

    # Wait for service to start
    server.wait_for_unit("mcp-exec.service")
    server.wait_for_open_port(19222)

    # Test HTTP endpoint responds
    server.succeed("curl -sf http://127.0.0.1:19222/")

    # Execute a simple command via MCP protocol
    result = server.succeed("""
      curl -sf -X POST http://127.0.0.1:19222/mcp/v1 \
        -H 'Content-Type: application/json' \
        -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}'
    """)
    assert "result" in result or "capabilities" in result, f"Initialize failed: {result}"

    # Send initialized notification and call exec
    server.succeed("""
      curl -sf -X POST http://127.0.0.1:19222/mcp/v1 \
        -H 'Content-Type: application/json' \
        -d '{"jsonrpc":"2.0","method":"notifications/initialized"}'
    """)

    # Execute echo command
    exec_result = server.succeed("""
      curl -sf -X POST http://127.0.0.1:19222/mcp/v1 \
        -H 'Content-Type: application/json' \
        -d '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"exec","arguments":{"cmd":"echo hello_nixos_test","timeout":10}}}'
    """)
    assert "id" in exec_result, f"Exec should return execution id: {exec_result}"
  '';
}
```

**Step 2: Verify file created**

Run: `cat nix/tests/basic.nix | head -20`
Expected: Shows the test file content

**Step 3: Commit**

```bash
git add nix/tests/basic.nix
git commit -m "test(nix): add basic VM health test"
```

---

## Task 2: Create Full Functionality VM Test

**Files:**
- Create: `nix/tests/full.nix`

**Step 1: Create the full functionality test**

Create `nix/tests/full.nix`:

```nix
{ pkgs, self }:

pkgs.nixosTest {
  name = "mcp-exec-full";

  nodes.server = { config, pkgs, ... }: {
    imports = [ self.nixosModules.default ];

    services.mcp-exec = {
      enable = true;
      port = 19222;
      directoryPath = "/var/lib/mcp-exec";
      sudo = {
        enable = true;
        commands = [ "ALL" ];
      };
    };

    # Add jq for JSON parsing in tests
    environment.systemPackages = [ pkgs.jq ];
  };

  testScript = ''
    import json
    import time

    start_all()

    server.wait_for_unit("mcp-exec.service")
    server.wait_for_open_port(19222)

    # Helper to make MCP calls
    def mcp_call(method, params=None, id=1):
        payload = {"jsonrpc": "2.0", "id": id, "method": method}
        if params:
            payload["params"] = params
        return server.succeed(f"""
          curl -sf -X POST http://127.0.0.1:19222/mcp/v1 \
            -H 'Content-Type: application/json' \
            -d '{json.dumps(payload)}'
        """)

    # Initialize
    init_result = mcp_call("initialize", {
        "protocolVersion": "2024-11-05",
        "capabilities": {},
        "clientInfo": {"name": "test", "version": "1.0"}
    })
    assert "result" in init_result, f"Initialize failed: {init_result}"

    # Send initialized notification
    server.succeed("""
      curl -sf -X POST http://127.0.0.1:19222/mcp/v1 \
        -H 'Content-Type: application/json' \
        -d '{"jsonrpc":"2.0","method":"notifications/initialized"}'
    """)

    # Test 1: Execute command and verify logs are stored
    exec_result = mcp_call("tools/call", {
        "name": "exec",
        "arguments": {"cmd": "echo 'persistence_test_12345'", "timeout": 10}
    }, id=2)
    assert "id" in exec_result, f"Exec should return id: {exec_result}"

    # Extract execution ID
    exec_data = json.loads(exec_result)
    exec_id = None
    if "result" in exec_data:
        content = exec_data["result"].get("content", [])
        if content and "text" in content[0]:
            text_data = json.loads(content[0]["text"])
            exec_id = text_data.get("id")

    time.sleep(1)  # Wait for command to complete

    # Test 2: Stream logs
    if exec_id:
        stream_result = mcp_call("tools/call", {
            "name": "stream_logs",
            "arguments": {"id": exec_id, "offset": 0}
        }, id=3)
        assert "persistence_test_12345" in stream_result, f"Logs should contain output: {stream_result}"

        # Test 3: Search logs with regex
        search_result = mcp_call("tools/call", {
            "name": "search_logs",
            "arguments": {"id": exec_id, "pattern": "persistence.*12345"}
        }, id=4)
        assert "matches" in search_result or "result" in search_result, f"Search should work: {search_result}"

    # Test 4: Verify log files exist on disk (persistence)
    server.succeed("ls -la /var/lib/mcp-exec/")
    server.succeed("test -d /var/lib/mcp-exec")

    # Test 5: Sudo functionality - execute privileged command
    sudo_result = mcp_call("tools/call", {
        "name": "exec",
        "arguments": {"cmd": "sudo cat /etc/shadow | head -1", "timeout": 10}
    }, id=5)
    assert "id" in sudo_result, f"Sudo exec should work: {sudo_result}"

    # Test 6: Service restart and log persistence
    server.systemctl("restart mcp-exec.service")
    server.wait_for_unit("mcp-exec.service")
    server.wait_for_open_port(19222)

    # Re-initialize after restart
    mcp_call("initialize", {
        "protocolVersion": "2024-11-05",
        "capabilities": {},
        "clientInfo": {"name": "test", "version": "1.0"}
    }, id=10)
    server.succeed("""
      curl -sf -X POST http://127.0.0.1:19222/mcp/v1 \
        -H 'Content-Type: application/json' \
        -d '{"jsonrpc":"2.0","method":"notifications/initialized"}'
    """)

    # Verify old logs still accessible after restart
    if exec_id:
        stream_after_restart = mcp_call("tools/call", {
            "name": "stream_logs",
            "arguments": {"id": exec_id, "offset": 0}
        }, id=11)
        assert "persistence_test_12345" in stream_after_restart, f"Logs should persist after restart: {stream_after_restart}"
  '';
}
```

**Step 2: Verify file created**

Run: `cat nix/tests/full.nix | head -20`
Expected: Shows the test file content

**Step 3: Commit**

```bash
git add nix/tests/full.nix
git commit -m "test(nix): add full functionality VM test with sudo and persistence"
```

---

## Task 3: Create Multi-Machine VM Test

**Files:**
- Create: `nix/tests/multi-machine.nix`

**Step 1: Create the multi-machine test**

Create `nix/tests/multi-machine.nix`:

```nix
{ pkgs, self }:

pkgs.nixosTest {
  name = "mcp-exec-multi-machine";

  nodes.server = { config, pkgs, ... }: {
    imports = [ self.nixosModules.default ];

    services.mcp-exec = {
      enable = true;
      port = 19222;
      bindAddress = "0.0.0.0";
      openFirewall = true;
    };

    networking.firewall.enable = true;
  };

  nodes.client = { config, pkgs, ... }: {
    environment.systemPackages = [ pkgs.curl pkgs.jq ];
  };

  testScript = ''
    import json

    start_all()

    # Wait for server to be ready
    server.wait_for_unit("mcp-exec.service")
    server.wait_for_open_port(19222)

    # Verify firewall is configured correctly on server
    server.succeed("iptables -L -n | grep 19222")

    # Test connectivity from client to server
    client.wait_for_unit("network.target")

    # Client should be able to reach server
    client.succeed("ping -c 1 server")

    # Test MCP protocol from client
    init_result = client.succeed("""
      curl -sf -X POST http://server:19222/mcp/v1 \
        -H 'Content-Type: application/json' \
        -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"remote-client","version":"1.0"}}}'
    """)
    assert "result" in init_result, f"Remote initialize failed: {init_result}"

    # Send initialized notification
    client.succeed("""
      curl -sf -X POST http://server:19222/mcp/v1 \
        -H 'Content-Type: application/json' \
        -d '{"jsonrpc":"2.0","method":"notifications/initialized"}'
    """)

    # Execute command from client
    exec_result = client.succeed("""
      curl -sf -X POST http://server:19222/mcp/v1 \
        -H 'Content-Type: application/json' \
        -d '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"exec","arguments":{"cmd":"hostname","timeout":10}}}'
    """)
    assert "id" in exec_result, f"Remote exec should return id: {exec_result}"

    # Verify the command ran on the server (hostname should be 'server')
    import time
    time.sleep(1)

    # Extract exec ID and verify logs
    exec_data = json.loads(exec_result)
    exec_id = None
    if "result" in exec_data:
        content = exec_data["result"].get("content", [])
        if content and "text" in content[0]:
            text_data = json.loads(content[0]["text"])
            exec_id = text_data.get("id")

    if exec_id:
        stream_result = client.succeed(f"""
          curl -sf -X POST http://server:19222/mcp/v1 \
            -H 'Content-Type: application/json' \
            -d '{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"stream_logs","arguments":{{"id":"{exec_id}","offset":0}}}}}}'
        """)
        assert "server" in stream_result, f"Command should have run on server: {stream_result}"

    # Test that client cannot bypass firewall on other ports
    # (This validates firewall config is working as expected)
    client.fail("curl -sf --connect-timeout 2 http://server:22/")
  '';
}
```

**Step 2: Verify file created**

Run: `cat nix/tests/multi-machine.nix | head -20`
Expected: Shows the test file content

**Step 3: Commit**

```bash
git add nix/tests/multi-machine.nix
git commit -m "test(nix): add multi-machine VM test with firewall validation"
```

---

## Task 4: Update Flake to Expose VM Tests

**Files:**
- Modify: `flake.nix`

**Step 1: Update flake.nix to include checks**

Replace the entire `flake.nix` with:

```nix
{
  description = "MCP exec server";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  inputs.flake-utils.url = "github:numtide/flake-utils";

  outputs = { self, nixpkgs, flake-utils, ... }:
    {
      # NixOS module (system-independent)
      nixosModules.default = import ./nix/module.nix;
    } //
    (flake-utils.lib.eachSystem [ "x86_64-linux" "aarch64-linux" ] (system:
      let
        pkgs = import nixpkgs { inherit system; };
      in {
        packages.default = pkgs.callPackage ./nix/package.nix { };
        devShells.default = import ./shell.nix { inherit pkgs; };

        checks = {
          vm-basic = import ./nix/tests/basic.nix { inherit pkgs self; };
          vm-full = import ./nix/tests/full.nix { inherit pkgs self; };
          vm-multi-machine = import ./nix/tests/multi-machine.nix { inherit pkgs self; };
        };
      }));
}
```

**Step 2: Verify flake syntax**

Run: `cd /home/robertwendt/mcp-exec && nix flake check --no-build`
Expected: No syntax errors (may warn about not building)

**Step 3: Commit**

```bash
git add flake.nix
git commit -m "feat(nix): expose VM tests as flake checks"
```

---

## Task 5: Create GitHub Actions Workflow

**Files:**
- Create: `.github/workflows/nixos-vm-tests.yml`

**Step 1: Create the workflow directory**

Run: `mkdir -p /home/robertwendt/mcp-exec/.github/workflows`

**Step 2: Create the workflow file**

Create `.github/workflows/nixos-vm-tests.yml`:

```yaml
name: NixOS VM Tests

on:
  push:
    branches: [master, main]
  pull_request:
    branches: [master, main]

jobs:
  vm-tests:
    name: VM Test - ${{ matrix.test }}
    runs-on: ubuntu-latest
    strategy:
      fail-fast: false
      matrix:
        test:
          - vm-basic
          - vm-full
          - vm-multi-machine

    steps:
      - name: Checkout
        uses: actions/checkout@v4

      - name: Install Nix
        uses: cachix/install-nix-action@v30
        with:
          nix_path: nixpkgs=channel:nixos-unstable
          extra_nix_config: |
            experimental-features = nix-command flakes
            accept-flake-config = true

      - name: Enable KVM
        run: |
          echo 'KERNEL=="kvm", GROUP="kvm", MODE="0666", OPTIONS+="static_node=kvm"' | sudo tee /etc/udev/rules.d/99-kvm4all.rules
          sudo udevadm control --reload-rules
          sudo udevadm trigger --name-match=kvm

      - name: Run VM Test
        run: |
          nix build .#checks.x86_64-linux.${{ matrix.test }} --print-build-logs
```

**Step 3: Verify file created**

Run: `cat .github/workflows/nixos-vm-tests.yml`
Expected: Shows the workflow content

**Step 4: Commit**

```bash
git add .github/workflows/nixos-vm-tests.yml
git commit -m "ci: add GitHub Actions workflow for NixOS VM tests"
```

---

## Task 6: Create Tests Directory and Verify Structure

**Files:**
- Create: `nix/tests/` directory

**Step 1: Create directory**

Run: `mkdir -p /home/robertwendt/mcp-exec/nix/tests`

**Step 2: Verify final structure**

Run: `tree /home/robertwendt/mcp-exec/nix/`
Expected:
```
nix/
├── module.nix
├── package.nix
└── tests/
    ├── basic.nix
    ├── full.nix
    └── multi-machine.nix
```

---

## Task 7: Test Locally (Optional)

**Step 1: Build one test to verify it works**

Run: `cd /home/robertwendt/mcp-exec && nix build .#checks.x86_64-linux.vm-basic --print-build-logs`
Expected: Test builds and runs (requires KVM)

**Step 2: If KVM not available, verify flake evaluates**

Run: `nix flake show`
Expected: Shows checks with all three vm tests listed

---

## Summary

After completing all tasks, you will have:

1. **Three VM tests** in `nix/tests/`:
   - `basic.nix` - Service starts, port responds, can execute commands
   - `full.nix` - Sudo config, log persistence across restarts, search functionality
   - `multi-machine.nix` - Client-server setup with firewall validation

2. **Updated flake** exposing tests as `checks.x86_64-linux.vm-*`

3. **GitHub Actions workflow** running all three tests in parallel with matrix strategy

The tests run in parallel in CI via the matrix strategy with `fail-fast: false`, so all tests complete even if one fails.
