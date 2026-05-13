{
  description = "rust-xv6 development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        riscv = pkgs.pkgsCross.riscv64-embedded.buildPackages;
        devPackages = [
          pkgs.gnumake
          pkgs.bc
          pkgs.perl
          pkgs.qemu
          pkgs.gdb
          pkgs.git
          pkgs.rustup
          # NOTE: kernel/user とも完全に Rust 化されており、`CC` (gcc) は
          # Makefile から呼ばれない (Makefile 冒頭コメント参照)。よって
          # 重量級のクロス gcc は不要で、binutils (ld / objdump) のみで足りる。
          # これにより macOS で gcc がキャッシュヒットせずソースビルドされて
          # 20 分かかる問題を回避する。
          riscv.binutils
        ];
        # Single closure that aggregates every devShell dependency so we can
        # register one GC root for the whole set.
        devEnv = pkgs.buildEnv {
          name = "xv6-rust-dev-env";
          paths = devPackages;
        };
      in {
        devShells.default = pkgs.mkShell {
          packages = devPackages;

          shellHook = ''
            # Pin devShell dependencies as a GC root so subsequent
            # `nix develop` invocations don't re-download anything.
            ROOT="$(git rev-parse --show-toplevel 2>/dev/null || echo "$PWD")"
            mkdir -p "$ROOT/.nix"
            nix-store --add-root "$ROOT/.nix/dev-profile" \
                      --indirect --realise ${devEnv} >/dev/null

            # binutils (objdump) を検出して TOOLPREFIX を決める。
            # gcc は持ち込んでいないので gcc では判定しない。
            if command -v riscv64-none-elf-objdump >/dev/null 2>&1; then
              export TOOLPREFIX=riscv64-none-elf-
            elif command -v riscv64-unknown-elf-objdump >/dev/null 2>&1; then
              export TOOLPREFIX=riscv64-unknown-elf-
            fi

            echo "rust-xv6 nix shell ready"
            echo "TOOLPREFIX=''${TOOLPREFIX:-auto-detect by Makefile}"
            echo "Run once if needed: rustup target add riscv64gc-unknown-none-elf"
          '';
        };
      });
}
