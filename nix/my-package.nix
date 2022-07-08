{ pkgs
}:
let
  shell = pkgs.mkShell {
    packages = with pkgs; [
      glibc
      gcc
      rustup
      rust-analyzer
    ];
  };
  src = pkgs.lib.sourceByRegex ../. [
    "Cargo.toml"
    "Cargo.lock"
    "src.*"
  ];

  pkg =
    pkgs.rustPlatform.buildRustPackage rec {
      inherit src;
      pname = "myfzf-wrapper-rs";
      version = "0.1.0";
      cargoLock = {
        lockFile = ../Cargo.lock;
      };
    };
in
pkg
