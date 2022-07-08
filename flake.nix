{
  description = "TODO";

  inputs = {
    #nixpkgs.url = "github:nixos/nixpkgs/nixpkgs-unstable";
    nixpkgs.url = "github:nixos/nixpkgs/master";
    flake-utils.url = "github:numtide/flake-utils/master";
    flake-compat = {
      url = github:edolstra/flake-compat;
      flake = false;
    };
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
        defaultPackage = pkgs.my-package;
        devShell = pkgs.my-shell;
      }
    );
}
