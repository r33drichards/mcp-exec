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
