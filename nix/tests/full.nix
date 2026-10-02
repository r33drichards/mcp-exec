{ pkgs, self }:

let
  # Python environment with MCP SDK
  pythonEnv = pkgs.python3.withPackages (ps: [
    ps.mcp
    ps.httpx
    ps.httpx-sse
  ]);

  # Test script for full functionality
  testScript = pkgs.writeScript "mcp-full-test.py" ''
    #!${pythonEnv}/bin/python3
    import asyncio
    import json
    import sys
    from mcp import ClientSession
    from mcp.client.streamable_http import streamablehttp_client

    async def test():
        async with streamablehttp_client("http://127.0.0.1:19222/") as (read, write, _):
            async with ClientSession(read, write) as session:
                await session.initialize()
                print("OK: initialize", file=sys.stderr)

                # Execute command
                result = await session.call_tool("exec", {
                    "bin": "echo",
                    "args": ["persistence_test_12345"],
                    "timeout": 10
                })
                exec_data = json.loads(result.content[0].text)
                exec_id = exec_data["id"]
                print(f"OK: exec id={exec_id}", file=sys.stderr)

                await asyncio.sleep(1)

                # Stream logs
                result = await session.call_tool("stream_logs", {
                    "id": exec_id,
                    "offset": 0
                })
                logs = result.content[0].text
                assert "persistence_test_12345" in logs, f"Output not found: {logs}"
                print("OK: stream_logs", file=sys.stderr)

                # Search logs
                result = await session.call_tool("search_logs", {
                    "id": exec_id,
                    "pattern": "persistence.*12345"
                })
                print("OK: search_logs", file=sys.stderr)

                # Test sudo command
                result = await session.call_tool("exec", {
                    "bin": "sh",
                    "args": ["-c", "sudo cat /etc/shadow | head -1"],
                    "timeout": 10
                })
                sudo_data = json.loads(result.content[0].text)
                print(f"OK: sudo exec id={sudo_data['id']}", file=sys.stderr)

                print("All tests passed!")
                return exec_id

    asyncio.run(test())
  '';

  # Restart test script
  restartScript = pkgs.writeScript "mcp-restart-test.py" ''
    #!${pythonEnv}/bin/python3
    import asyncio
    from mcp import ClientSession
    from mcp.client.streamable_http import streamablehttp_client

    async def test():
        async with streamablehttp_client("http://127.0.0.1:19222/") as (read, write, _):
            async with ClientSession(read, write) as session:
                await session.initialize()
                print("OK: reconnect after restart")

    asyncio.run(test())
  '';
in
pkgs.testers.nixosTest {
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

    environment.systemPackages = [ pythonEnv ];
  };

  testScript = ''
    start_all()

    server.wait_for_unit("mcp-exec.service")
    server.wait_for_open_port(19222)

    # Test using Python MCP client
    server.succeed("${testScript}")

    # Verify log files exist on disk
    server.succeed("ls -la /var/lib/mcp-exec/")
    server.succeed("test -d /var/lib/mcp-exec")

    # Test service restart and log persistence
    server.systemctl("restart mcp-exec.service")
    server.wait_for_unit("mcp-exec.service")
    server.wait_for_open_port(19222)

    # Verify logs persist after restart (basic connectivity test)
    server.succeed("${restartScript}")
  '';
}
