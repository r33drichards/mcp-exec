{ pkgs, self }:

let
  # Python environment with MCP SDK
  pythonEnv = pkgs.python3.withPackages (ps: [
    ps.mcp
    ps.httpx
    ps.httpx-sse
  ]);

  # Test client script
  testClient = pkgs.writeScript "mcp-test-client" ''
    #!${pythonEnv}/bin/python3
    ${builtins.readFile ./mcp_test_client.py}
  '';
in
pkgs.testers.nixosTest {
  name = "mcp-exec-basic";

  nodes.server = { config, pkgs, ... }: {
    imports = [ self.nixosModules.default ];

    services.mcp-exec = {
      enable = true;
      port = 19222;
    };

    environment.systemPackages = [ pythonEnv ];
  };

  testScript = ''
    start_all()

    # Wait for service to start
    server.wait_for_unit("mcp-exec.service")
    server.wait_for_open_port(19222)

    # Run MCP test client
    server.succeed("${testClient} --url http://127.0.0.1:19222/")
  '';
}
