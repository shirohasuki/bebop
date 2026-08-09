{ pkgs, base }:

let
  pkUrl = "https://github.com/riscv-software-src/riscv-pk.git";
  pkRev = "9c61d29846d8521d9487a57739330f9682d5b542";
  pkSrc = builtins.fetchGit {
    url = pkUrl;
    rev = pkRev;
  };
in
rec {
  riscvGcc = base.riscvGcc;
  riscvBinutils = base.riscvBinutils;

  pkDrv = pkgs.stdenv.mkDerivation {
    pname = "riscv-pk";
    version = pkRev;
    src = pkSrc;

    nativeBuildInputs = [
      base.gnumake
      base.autoconf
      base.automake
      base.libtool
      base.pkgConfig
    ];

    buildInputs = [
      base.riscvGcc
      base.riscvBinutils
    ];

    dontConfigure = true;

    buildPhase = ''
      runHook preBuild
      mkdir -p build
      cd build

      export CC="${base.riscvGcc}/bin/riscv64-none-elf-gcc"
      export PATH="${base.riscvGcc}/bin:${base.riscvBinutils}/bin:$PATH"
      export OBJCOPY="${base.riscvBinutils}/bin/riscv64-none-elf-objcopy"
      export READELF="${base.riscvBinutils}/bin/riscv64-none-elf-readelf"
      host="$($CC -dumpmachine)"

      ../configure --prefix="$out" --host="$host" ac_cv_prog_cc_cross=yes

      if [ -f Makefile ]; then
        sed -i 's/^\([[:space:]]*march := -march=\).*/\1rv64gc_zicsr_zifencei/' Makefile
        sed -i 's/^\([[:space:]]*mabi := -mabi=\).*/\1lp64/' Makefile
      fi

      # Newlib's assert.h can be included after pk's mtrap.h and overwrite
      # pk's freestanding assert macro. Rename pk's private assertion API in
      # the temporary source tree so it cannot collide with libc headers.
      find ../machine ../pk ../bbl -type f \( -name '*.c' -o -name '*.h' \) \
        -exec sed -i 's/\<assert[[:space:]]*(/pk_assert(/g' {} +

      make -j"$NIX_BUILD_CORES" march=-march=rv64gc_zicsr_zifencei mabi=-mabi=lp64
      make install

      mkdir -p "$out/bin"
      realPk="$out/riscv64-none-elf/bin/pk"
      if [ ! -x "$realPk" ]; then
        echo "ERROR: pk not found under $out after install"
        exit 1
      fi
      unknownDir="$out/riscv64-unknown-elf/bin"
      mkdir -p "$unknownDir"
      unknownPk="$unknownDir/pk"
      ln -sf "$realPk" "$unknownPk"
      if [ ! -x "$unknownPk" ]; then
        echo "ERROR: failed to create unknown-elf pk link"
        exit 1
      fi
      ln -sf "$unknownPk" "$out/bin/pk"
      runHook postBuild
    '';

    installPhase = ''
      runHook preInstall
      true
      runHook postInstall
    '';
  };

  buildInputs = [
    base.riscvGcc
    base.riscvBinutils
    base.riscvLinuxGcc
    pkDrv
  ];

  shellHook = ''
    # echo "riscv gcc: $(command -v riscv64-none-elf-gcc)"
    # echo "pk: $(command -v pk)"
  '';
}
