{
  description = "TODO";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixpkgs-unstable";
    nixpkgs-old.url = "github:nixos/nixpkgs/6141b8932a5cf376fe18fcd368cecd9ad946cb68";
    flake-utils.url = "github:numtide/flake-utils/main";
  };

  outputs = { nixpkgs, nixpkgs-old, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        pkgs-old = import nixpkgs-old { inherit system; };

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
                ${pkgs-old.ripgrep}/bin:${pkgs-old.fzf}/bin
          '';
        };

      in
      {
        packages.default = fzfw;
        devShells.default = shell;
      }
    );
}
