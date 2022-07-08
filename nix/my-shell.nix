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
in
shell
