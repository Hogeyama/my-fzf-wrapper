{
  description = "TODO";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/master";
    flake-utils.url = "github:numtide/flake-utils/master";
  };

  outputs = { self, nixpkgs, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };

        shell = pkgs.mkShell {
          packages = with pkgs; [
            glibc
            gcc
            rustup
            rust-analyzer
          ];
        };

        fzfw = pkgs.rustPlatform.buildRustPackage {
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
          buildInputs = [ pkgs.makeWrapper ];
          postFixup = ''
            wrapProgram $out/bin/fzfw \
              --prefix PATH : \
                ${pkgs.ripgrep}/bin:${pkgs.fzf}/bin
          '';
        };

      in
      {
        packages.default = fzfw;
        devShells.default = shell;
      }
    );
}
