# NixOS Module Design for mcp-exec

## Overview

A NixOS module to run mcp-exec as a system service with configurable options for sandboxed deployments.

## Requirements

- Run as a dedicated system user with optional sudo access
- HTTP transport mode by default (for persistent service)
- File-based log storage
- Bind to `0.0.0.0` for sandboxed environments
- Default port: 19222

## Module Options

```nix
services.mcp-exec = {
  enable = true;                          # Enable the service
  user = "mcp-exec";                      # User to run as
  group = "mcp-exec";                     # Group to run as
  port = 19222;                           # HTTP port (default: 19222)
  bindAddress = "0.0.0.0";                # Bind address (default: 0.0.0.0)
  directoryPath = "/var/lib/mcp-exec";    # Log storage path
  package = pkgs.mcp-exec;                # Package to use
  extraArgs = [];                         # Additional CLI arguments
  openFirewall = false;                   # Open firewall port

  # Sudo configuration
  sudo = {
    enable = false;                       # Grant passwordless sudo (default: false)
    commands = [ "ALL" ];                 # Allowed commands (default: ALL)
  };
};
```

## Systemd Service

```nix
systemd.services.mcp-exec = {
  description = "MCP Exec - Shell Command Execution Server";
  wantedBy = [ "multi-user.target" ];
  after = [ "network.target" ];

  serviceConfig = {
    Type = "simple";
    User = cfg.user;
    Group = cfg.group;
    ExecStart = "${cfg.package}/bin/mcp-exec --http-port ${toString cfg.port} --directory-path ${cfg.directoryPath}";
    Restart = "on-failure";
    RestartSec = 5;

    # State directory (auto-creates /var/lib/mcp-exec with correct ownership)
    StateDirectory = "mcp-exec";

    # Minimal hardening (permissive since we execute arbitrary commands)
    NoNewPrivileges = false;  # Needed for sudo
    ProtectSystem = "false";  # Commands may need system access
    ProtectHome = "false";    # Commands may need home access
  };
};
```

## Package Derivation

```nix
{ lib, rustPlatform, pkg-config, openssl }:

rustPlatform.buildRustPackage {
  pname = "mcp-exec";
  version = "0.1.0";

  src = ./..;

  cargoLock = {
    lockFile = ../Cargo.lock;
    outputHashes = {
      "rmcp-0.12.0" = "sha256-GaZGW3I95DJnkoQrmehtqFGEP0xibnqXyapby2LFtok=";
    };
  };

  nativeBuildInputs = [ pkg-config ];
  buildInputs = [ openssl ];

  meta = with lib; {
    description = "MCP server for async shell command execution";
    license = licenses.mit;
    mainProgram = "mcp-exec";
  };
}
```

## Flake Structure

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    {
      nixosModules.default = import ./nix/module.nix;
    } //
    (flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
      in {
        packages.default = pkgs.callPackage ./nix/package.nix { };
        devShells.default = import ./shell.nix { inherit pkgs; };
      }));
}
```

## File Structure

```
mcp-exec/
├── flake.nix           # Updated to export module and package
├── nix/
│   ├── module.nix      # NixOS module definition
│   └── package.nix     # Package derivation
└── docs/
    └── plans/
        └── 2026-01-04-nixos-module-design.md
```

## Usage

### Adding to a NixOS configuration

```nix
# flake.nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    mcp-exec.url = "github:youruser/mcp-exec";  # or path:/path/to/mcp-exec
  };

  outputs = { self, nixpkgs, mcp-exec, ... }: {
    nixosConfigurations.myhost = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        mcp-exec.nixosModules.default
        {
          services.mcp-exec = {
            enable = true;
            sudo.enable = true;
          };
        }
      ];
    };
  };
}
```

### Configuration Examples

**Basic (defaults):**
```nix
services.mcp-exec.enable = true;
```

**With sudo and custom port:**
```nix
services.mcp-exec = {
  enable = true;
  port = 8080;
  sudo.enable = true;
};
```

**Restricted sudo commands:**
```nix
services.mcp-exec = {
  enable = true;
  sudo = {
    enable = true;
    commands = [ "/run/current-system/sw/bin/systemctl" "/run/current-system/sw/bin/journalctl" ];
  };
};
```

**Running as existing user:**
```nix
services.mcp-exec = {
  enable = true;
  user = "myuser";
  group = "mygroup";
};
```
