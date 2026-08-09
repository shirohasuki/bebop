use std::path::{Path, PathBuf};

pub fn emit(manifest_dir: &Path, native_dir: &Path, topology_files: Vec<PathBuf>, chip_ball_files: Vec<PathBuf>) {
    let build_script_dir = native_dir.parent().expect("BEMU native dir has parent");

    for path in [
        "build.rs",
        "build_support/mod.rs",
        "build_support/chip.rs",
        "build_support/config_loader.rs",
        "build_support/rerun.rs",
        "build_support/spike.rs",
    ] {
        println!("cargo:rerun-if-changed={}", build_script_dir.join(path).display());
    }

    for path in [
        manifest_dir.join("Cargo.toml"),
        manifest_dir.join("src/lib.rs"),
        manifest_dir.join("src/chip.rs"),
    ] {
        println!("cargo:rerun-if-changed={}", path.display());
    }

    for path in topology_files {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    for path in chip_ball_files {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    for path in ["rocc.cc", "spike.cc", "btif.cc", "btif.h"] {
        println!("cargo:rerun-if-changed={}", native_dir.join(path).display());
    }
    println!("cargo:rerun-if-changed={}", native_dir.join("spike").display());
}
