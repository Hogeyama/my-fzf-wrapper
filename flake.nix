{
  description = "TODO";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixpkgs-unstable";
    nixpkgs-old.url = "github:nixos/nixpkgs/6141b8932a5cf376fe18fcd368cecd9ad946cb68";
    fenix.url = "github:nix-community/fenix";
    fenix.inputs.nixpkgs.follows = "nixpkgs";
    naersk.url = "github:nix-community/naersk";
    naersk.inputs.nixpkgs.follows = "nixpkgs";
    flake-utils.url = "github:numtide/flake-utils/main";
  };

  outputs = { nixpkgs, nixpkgs-old, naersk, fenix, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        pkgs-old = import nixpkgs-old { inherit system; };

        toolchain = with fenix.packages.${system}; combine [
          complete.cargo
          complete.clippy
          complete.rust-analyzer
          complete.rust-src
          complete.rustc
          complete.rustfmt
        ];

        naerskLib = naersk.lib.${system}.override {
          cargo = toolchain;
          rustc = toolchain;
        };

        shell = pkgs.mkShell {
          packages = [
            toolchain
          ];
          CARGO_FPATH = "${toolchain}/share/zsh/site-functions/";
        };

        fzfw-unwrapped = naerskLib.buildPackage { src = ./.; };
        fzfw = pkgs.runCommandCC "fzfw"
          { buildInputs = [ pkgs.makeWrapper fzfw-unwrapped ]; }
          ''
            set -x
            mkdir -p $out/bin
            cp ${fzfw-unwrapped}/bin/fzfw $out/bin/fzfw
            wrapProgram $out/bin/fzfw \
              --prefix PATH : ${pkgs-old.fzf}/bin \
              --prefix PATH : ${pkgs-old.ripgrep}/bin \
          '';
      in
      {
        packages = {
          default = fzfw;
          inherit fzfw fzfw-unwrapped;
        };
        devShells.default = shell;
      }
    );
}
