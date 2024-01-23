{
  description = "Hogeyama's fzf wrapper";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixpkgs-unstable";
    # fzf等のバイナリを取り込むためのnixpkgs
    # 互換性のために特定のコミットを指定したくなることがあるため分けている。
    nixpkgs-for-bin.url = "github:nixos/nixpkgs/nixpkgs-unstable";
    fenix.url = "github:nix-community/fenix";
    fenix.inputs.nixpkgs.follows = "nixpkgs";
    naersk.url = "github:nix-community/naersk";
    naersk.inputs.nixpkgs.follows = "nixpkgs";
    flake-utils.url = "github:numtide/flake-utils/main";
  };

  outputs = { nixpkgs, nixpkgs-for-bin, naersk, fenix, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        pkgs-for-bin = import nixpkgs-for-bin { inherit system; };

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
            pkgs.pkg-config
            pkgs.openssl.dev
          ];
          CARGO_FPATH = "${toolchain}/share/zsh/site-functions/";
          # `MANPATH=$FZF_MANPATH man fzf` でこのバージョンのfzfのマニュアルを見る
          FZF_MANPATH = "${pkgs-for-bin.fzf.man}/share/man";
        };

        fzfw-unwrapped = naerskLib.buildPackage {
          name = "fzfw-unwrapped";
          src = ./.;
          buildInputs = [
            pkgs.pkg-config
            pkgs.openssl.dev
          ];
        };
        fzfw = pkgs.runCommand "fzfw"
          { buildInputs = [ pkgs.makeWrapper fzfw-unwrapped ]; }
          ''
            set -x
            mkdir -p $out/bin
            cp ${fzfw-unwrapped}/bin/fzfw $out/bin/fzfw
            wrapProgram $out/bin/fzfw \
              --prefix PATH : ${pkgs-for-bin.fzf}/bin \
              --prefix PATH : ${pkgs-for-bin.ripgrep}/bin \
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
