{ pkgs, self }:

let
  # Python environment with MCP SDK
  pythonEnv = pkgs.python3.withPackages (ps: [
    ps.mcp
    ps.httpx
    ps.httpx-sse
  ]);
in
pkgs.testers.nixosTest {
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
    environment.systemPackages = [ pythonEnv pkgs.curl ];
  };

  testScript = ''
    import time

    start_all()

    # Wait for server to be ready
    server.wait_for_unit("mcp-exec.service")
    server.wait_for_open_port(19222)

    # Verify firewall is configured correctly on server
    server.succeed("iptables -L -n | grep 19222")

    # Test connectivity from client to server
    client.wait_for_unit("network.target")
    client.succeed("ping -c 1 server")

    # Test MCP protocol from client using Python MCP SDK
    test_script = """
import asyncio
import json
import sys
from mcp import ClientSession
from mcp.client.streamable_http import streamablehttp_client

async def test():
    async with streamablehttp_client("http://server:19222/") as (read, write, _):
        async with ClientSession(read, write) as session:
            await session.initialize()
            print("OK: remote initialize", file=sys.stderr)

            # List tools to verify connection
            tools = await session.list_tools()
            print(f"OK: list_tools - {[t.name for t in tools.tools]}", file=sys.stderr)

            # Execute hostname command
            result = await session.call_tool("exec", {
                "cmd": "hostname",
                "timeout": 10
            })
            exec_data = json.loads(result.content[0].text)
            exec_id = exec_data["id"]
            print(f"OK: exec id={exec_id}", file=sys.stderr)

            await asyncio.sleep(1)

            # Stream logs and verify command ran on server
            result = await session.call_tool("stream_logs", {
                "id": exec_id,
                "offset": 0
            })
            logs = result.content[0].text
            assert "server" in logs, f"Command should have run on server: {logs}"
            print("OK: verified command ran on server", file=sys.stderr)

            print("All remote tests passed!")

asyncio.run(test())
"""
    client.succeed(f'${pythonEnv}/bin/python3 -c "{test_script}"')

    # Test that client cannot bypass firewall on other ports
    client.fail("curl -sf --connect-timeout 2 http://server:22/")
  '';
}
