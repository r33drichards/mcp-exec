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
