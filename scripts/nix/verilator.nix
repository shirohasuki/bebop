{ pkgs }:

{
  verilator = pkgs.verilator;

  dramsim3 = pkgs.stdenv.mkDerivation {
    pname = "dramsim3";
    version = "unstable-2026-07-23";

    src = pkgs.fetchFromGitHub {
      owner = "umd-memsys";
      repo = "DRAMsim3";
      rev = "29817593b3389f1337235d63cac515024ab8fd6e";
      hash = "sha256-uErpWJEn6C9oKR6Bv1NOAC3ij3ne3A6BPtjtX7D8ZwE=";
    };

    nativeBuildInputs = [ pkgs.cmake ];
    dontUseCmakeConfigure = true;
    postPatch = ''
      substituteInPlace src/dramsim3.h \
        --replace-fail '#include <string>' '#include <string>
#include <stdint.h>'
    '';

    buildPhase = ''
      runHook preBuild
      cmake -S . -B build -DCMAKE_BUILD_TYPE=Release -DCMAKE_POLICY_VERSION_MINIMUM=3.5
      cmake --build build --target dramsim3
      runHook postBuild
    '';

    installPhase = ''
      runHook preInstall
      mkdir -p $out/lib $out/include $out/share/dramsim3
      cp libdramsim3.so $out/lib/
      cp src/*.h ext/headers/*.h ext/headers/*.hpp $out/include/
      cp -r configs $out/share/dramsim3/
      runHook postInstall
    '';

    meta = {
      description = "DRAMsim3 memory system simulator";
      homepage = "https://github.com/umd-memsys/DRAMsim3";
    };
  };
}
