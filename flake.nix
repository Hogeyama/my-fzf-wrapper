{
  description = "Hogeyama's fzf wrapper";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixpkgs-unstable";
    # fzf等のバイナリを取り込むためのnixpkgs
    # 互換性のために特定のコミットを指定したくなることがあるため分けている。
    nixpkgs-for-bin.url = "github:nixos/nixpkgs/nixpkgs-unstable";
    # この次の日5/18のコミット49b387d269e31271666c01d42a2763f87bbe62b1を指定すると
    # RUNPATHが設定されなくて使えなくなる。
    fenix.url = "github:nix-community/fenix/a2d19ef9305841f26c8ab908b1c09a84ca307e18";
    fenix.inputs.nixpkgs.follows = "nixpkgs";
    naersk.url = "github:nix-community/naersk";
    naersk.inputs.nixpkgs.follows = "nixpkgs";
    flake-utils.url = "github:numtide/flake-utils/main";
  };

  outputs = { self, nixpkgs, nixpkgs-for-bin, naersk, fenix, flake-utils, ... }:
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

        fzfw-unwrapped = (naerskLib.buildPackage {
          name = "fzfw-unwrapped";
          src = ./.;
          buildInputs = [
            pkgs.pkg-config
            pkgs.openssl.dev
          ];
        }).overrideAttrs (oldAttrs: {
          # overrideAttrsに書かないと依存関係が毎回ビルドされてしまう
          # cf. https://github.com/nix-community/naersk?tab=readme-ov-file#note-on-overrideattrs
          GIT_REVISION = if self ? shortRev then self.shortRev else "dirty";
        });
        fzfw = pkgs.runCommand "fzfw"
          { buildInputs = [ pkgs.makeWrapper fzfw-unwrapped ]; }
          ''
            set -x
            mkdir -p $out/bin
            cp ${fzfw-unwrapped}/bin/fzfw $out/bin/fzfw
            wrapProgram $out/bin/fzfw \
              --prefix PATH : ${pkgs-for-bin.bat}/bin \
              --prefix PATH : ${pkgs-for-bin.eza}/bin \
              --prefix PATH : ${pkgs-for-bin.fd}/bin \
              --prefix PATH : ${pkgs-for-bin.fzf}/bin \
              --prefix PATH : ${pkgs-for-bin.gh}/bin \
              --prefix PATH : ${pkgs-for-bin.git}/bin \
              --prefix PATH : ${pkgs-for-bin.glow}/bin \
              --prefix PATH : ${pkgs-for-bin.lazygit}/bin \
              --prefix PATH : ${pkgs-for-bin.ripgrep}/bin \
              --prefix PATH : ${pkgs-for-bin.vifm}/bin \
              --prefix PATH : ${pkgs-for-bin.xsel}/bin \
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
