{ pkgs }:

let
  newlib-nano = pkgs.pkgsCross.riscv64-embedded.newlib.overrideAttrs (oldAttrs: {
    pname = "newlib-nano";
    configureFlags = oldAttrs.configureFlags or [] ++ [
      "--enable-newlib-nano-malloc"
      "--enable-newlib-nano-formatted-io"
      "--enable-newlib-reent-small"
      "--disable-newlib-fvwrite-in-streamio"
      "--disable-newlib-fseek-optimization"
      "--disable-newlib-wide-orient"
      "--disable-newlib-unbuf-stream-opt"
      "--enable-lite-exit"
      "--enable-newlib-global-atexit"
    ];
    CFLAGS_FOR_TARGET = "-Os -ffunction-sections -fdata-sections -mcmodel=medany";
  });

  riscv64EmbeddedWithNano = pkgs.pkgsCross.riscv64-embedded.stdenv.targetPlatform // {
    libc = "newlib-nano";
  };

  pkgsCrossWithNano = import pkgs.path {
    inherit (pkgs) system;
    crossSystem = riscv64EmbeddedWithNano;
    overlays = [
      (self: super: {
        newlib = newlib-nano;
      })
    ];
  };

  riscvEmbeddedGcc = pkgs.symlinkJoin {
    name = "riscv64-unknown-elf-gcc";
    paths = [ pkgsCrossWithNano.buildPackages.gcc ];
    postBuild = ''
      cd $out/bin
      for f in riscv64-none-elf-*; do
        [ -e "$f" ] || continue
        newname=''${f/riscv64-none-elf/riscv64-unknown-elf}
        ln -sf "$f" "$newname"
      done
    '';
  };

  riscvLinuxGcc = let
    cc = pkgs.pkgsCross.riscv64.stdenv.cc;
    libcStatic = pkgs.pkgsCross.riscv64.stdenv.cc.libc.static;
  in pkgs.runCommand "riscv64-linux-gnu-toolchain" {} ''
    mkdir -p $out/bin
    for f in ${cc}/bin/riscv64-unknown-linux-gnu-*; do
      [ -e "$f" ] || continue
      name=$(basename "$f")
      case "$name" in
        riscv64-unknown-linux-gnu-gcc|riscv64-unknown-linux-gnu-g++|riscv64-unknown-linux-gnu-c++)
          echo '#!${pkgs.stdenv.shell}' > $out/bin/$name
          echo 'exec "'"$f"'" -L${libcStatic}/lib "$@"' >> $out/bin/$name
          chmod +x $out/bin/$name
          ;;
        *)
          ln -s "$f" $out/bin/$name
          ;;
      esac
    done
  '';
in {
  autoconf = pkgs.autoconf;
  automake = pkgs.automake;
  libtool = pkgs.libtool;
  gnumake = pkgs.gnumake;
  pkgConfig = pkgs.pkg-config;
  cmake = pkgs.cmake;
  ninja = pkgs.ninja;
  dtc = pkgs.dtc;
  gcc = pkgs.gcc;
  clang = pkgs.clang;
  boost = pkgs.boost.dev;
  python3 = pkgs.python3;
  # clangTools = pkgs.clang-tools;
  cargo = pkgs.cargo;
  cargoNextest = pkgs.cargo-nextest;
  rustc = pkgs.rustc;
  rustfmt = pkgs.rustfmt;
  clippy = pkgs.clippy;
  preCommit = pkgs.pre-commit;

  riscvGcc = riscvEmbeddedGcc;
  riscvBinutils = pkgs.pkgsCross.riscv64-embedded.buildPackages.binutils;
  riscvLinuxGcc = riscvLinuxGcc;
}
