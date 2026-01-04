{ config, lib, pkgs, ... }:

let
  cfg = config.services.mcp-exec;
in
{
  options.services.mcp-exec = {
    enable = lib.mkEnableOption "MCP Exec server for async shell command execution";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.callPackage ./package.nix { };
      defaultText = lib.literalExpression "pkgs.callPackage ./package.nix { }";
      description = "The mcp-exec package to use.";
    };

    user = lib.mkOption {
      type = lib.types.str;
      default = "mcp-exec";
      description = "User account under which mcp-exec runs.";
    };

    group = lib.mkOption {
      type = lib.types.str;
      default = "mcp-exec";
      description = "Group under which mcp-exec runs.";
    };

    port = lib.mkOption {
      type = lib.types.port;
      default = 19222;
      description = "HTTP port to listen on.";
    };

    bindAddress = lib.mkOption {
      type = lib.types.str;
      default = "0.0.0.0";
      description = "Address to bind the HTTP server to.";
    };

    directoryPath = lib.mkOption {
      type = lib.types.path;
      default = "/var/lib/mcp-exec";
      description = "Directory for storing command logs and state.";
    };

    extraArgs = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      description = "Additional command-line arguments to pass to mcp-exec.";
    };

    openFirewall = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Whether to open the firewall port for mcp-exec.";
    };

    sudo = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = "Whether to grant the mcp-exec user passwordless sudo access.";
      };

      commands = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [ "ALL" ];
        description = "List of commands the mcp-exec user can run with sudo. Use ALL for unrestricted access.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    # Create user and group if using defaults
    users.users.${cfg.user} = lib.mkIf (cfg.user == "mcp-exec") {
      isSystemUser = true;
      group = cfg.group;
      home = cfg.directoryPath;
      description = "MCP Exec service user";
    };

    users.groups.${cfg.group} = lib.mkIf (cfg.group == "mcp-exec") { };

    # Systemd service
    systemd.services.mcp-exec = {
      description = "MCP Exec - Shell Command Execution Server";
      wantedBy = [ "multi-user.target" ];
      after = [ "network.target" ];

      serviceConfig = {
        Type = "simple";
        User = cfg.user;
        Group = cfg.group;
        ExecStart = lib.concatStringsSep " " ([
          "${cfg.package}/bin/mcp-exec"
          "--http-port" (toString cfg.port)
          "--directory-path" cfg.directoryPath
        ] ++ cfg.extraArgs);
        Restart = "on-failure";
        RestartSec = 5;

        # State directory
        StateDirectory = "mcp-exec";
        StateDirectoryMode = "0750";

        # Minimal hardening - permissive since we execute arbitrary commands
        NoNewPrivileges = false;
        ProtectSystem = "false";
        ProtectHome = "false";
      };
    };

    # Sudo configuration
    security.sudo.extraRules = lib.mkIf cfg.sudo.enable [
      {
        users = [ cfg.user ];
        commands = map (cmd: {
          command = cmd;
          options = [ "NOPASSWD" ];
        }) cfg.sudo.commands;
      }
    ];

    # Firewall
    networking.firewall.allowedTCPPorts = lib.mkIf cfg.openFirewall [ cfg.port ];
  };
}
