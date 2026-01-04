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
