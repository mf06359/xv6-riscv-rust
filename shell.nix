{ pkgs ? import <nixpkgs> { } }:
let
  riscv = pkgs.pkgsCross.riscv64-embedded.buildPackages;
in
pkgs.mkShell {
  packages = [
    pkgs.gnumake
    pkgs.bc
    pkgs.perl
    pkgs.qemu
    pkgs.gdb
    pkgs.git
    pkgs.rustup
    # kernel/user とも完全に Rust 化されているため CC (gcc) は不要。
    # binutils (ld/objdump) のみあれば Makefile はリンクと逆アセンブルができる。
    riscv.binutils
  ];

  shellHook = ''
    if command -v riscv64-none-elf-objdump >/dev/null 2>&1; then
      export TOOLPREFIX=riscv64-none-elf-
    elif command -v riscv64-unknown-elf-objdump >/dev/null 2>&1; then
      export TOOLPREFIX=riscv64-unknown-elf-
    fi

    echo "rust-xv6 nix shell ready"
    echo "TOOLPREFIX=''${TOOLPREFIX:-auto-detect by Makefile}"
    echo "Run once if needed: rustup target add riscv64gc-unknown-none-elf"
  '';
}
