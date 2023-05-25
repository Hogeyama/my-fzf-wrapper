{
  description = "TODO";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/master";
    flake-utils.url = "github:numtide/flake-utils/master";
  };

  outputs = { self, nixpkgs, flake-utils, ... }:
    let
      supportedSystems = [ "x86_64-linux" ];

      outputs-overlay = pkgs: prev: {
        my-shell = import ./nix/my-shell.nix { inherit pkgs; };
        my-package = import ./nix/my-package.nix { inherit pkgs; };
      };
    in
    flake-utils.lib.eachSystem supportedSystems (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ outputs-overlay ];
        };
      in
      {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "myfzf-wrapper-rs";
          version = "0.1.0";
          src = pkgs.lib.sourceByRegex ./. [
            "Cargo.toml"
            "Cargo.lock"
            "src.*"
          ];
          cargoLock = {
            lockFile = ./Cargo.lock;
          };
        };
        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            glibc
            gcc
            rustup
            rust-analyzer
          ];
        };
      }
    );
}
