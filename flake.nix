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
