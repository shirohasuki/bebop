{
  description = "bebop - A buckyball emulator written in Rust";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [
          (import ./scripts/nix/overlay.nix)
        ];
        pkgs = import nixpkgs { inherit system overlays; };
        preCommitCfg = ./scripts/tools/pre-commit-config.yaml;
        preCommitInstall = pkgs.writeShellApplication {
          name = "bebop-pre-commit-install";
          runtimeInputs = [ pkgs.base.preCommit ];
          text = ''
            exec pre-commit install --install-hooks --hook-type pre-commit -c ${preCommitCfg}
          '';
        };
      in
      {
        devShells.default = pkgs.mkShell {
          buildInputs = [
            pkgs.base.autoconf
            pkgs.base.automake
            pkgs.base.libtool
            pkgs.base.gnumake
            pkgs.base.pkgConfig
            # pkgs.base.clangTools
            pkgs.base.cmake
            pkgs.base.ninja
            pkgs.base.dtc
            pkgs.base.boost
            pkgs.base.python3
            pkgs.base.cargo
            pkgs.base.cargoNextest
            pkgs.base.rustc
            pkgs.base.rustfmt
            pkgs.base.clippy
            pkgs.base.preCommit

            pkgs.verilator
            pkgs.bebop
            pkgs.base.clang
            # Use gcc13 instead of gcc8 for P2E vvac builds
            pkgs.gcc13

            # P2E waveform tools
            pkgs.p2e.gtkwave
          ] ++ pkgs.riscv.buildInputs ++ pkgs.bemu.buildInputs;

          shellHook = ''
            export CC="${pkgs.base.clang}/bin/clang"
            export CXX="${pkgs.base.clang}/bin/clang++"
          '' + pkgs.riscv.shellHook + pkgs.bemu.shellHook + ''
            echo "================= bebop development environment activated ========================="
            echo "Enable nodes including:"
            echo "bebop: $(command -v bebop)"
            echo "riscv gcc: $(command -v riscv64-none-elf-gcc)"
            echo "verilator: $(command -v verilator)"
            echo "CXX: $CXX"
            echo "==========================================================================="
          '';
        };

        packages.default = pkgs.bebop;
        apps.pre-commit-install = {
          type = "app";
          program = "${preCommitInstall}/bin/bebop-pre-commit-install";
        };
      }
    );
}
