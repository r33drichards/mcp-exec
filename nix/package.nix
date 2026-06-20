{ lib
, rustPlatform
, pkg-config
, openssl
}:

rustPlatform.buildRustPackage {
  pname = "mcp-exec";
  version = "0.1.0";

  src = ./..;

  cargoLock = {
    lockFile = ../Cargo.lock;
  };

  nativeBuildInputs = [ pkg-config ];
  buildInputs = [ openssl ];

  # Skip tests during build (run separately if needed)
  doCheck = false;

  meta = with lib; {
    description = "MCP server for async shell command execution";
    homepage = "https://github.com/modelcontextprotocol/mcp-exec";
    license = licenses.mit;
    mainProgram = "mcp-exec";
  };
}
