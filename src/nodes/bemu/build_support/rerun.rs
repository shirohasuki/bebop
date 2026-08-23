use std::path::{Path, PathBuf};

pub fn emit_engine(engine_dir: &Path, native_dir: &Path) {
    for path in [
        "build.rs",
        "src/lib.rs",
        "src/chip.rs",
        "build_support/mod.rs",
        "build_support/spike.rs",
    ] {
        println!("cargo:rerun-if-changed={}", engine_dir.join(path).display());
    }
    println!("cargo:rerun-if-changed={}", engine_dir.join("Cargo.toml").display());
    for path in ["rocc.cc", "spike.cc", "btif.cc", "btif.h"] {
        println!("cargo:rerun-if-changed={}", native_dir.join(path).display());
    }
    println!("cargo:rerun-if-changed={}", native_dir.join("spike").display());
}

#[allow(dead_code)]
pub fn emit(manifest_dir: &Path, native_dir: &Path, topology_files: Vec<PathBuf>, chip_ball_files: Vec<PathBuf>) {
    emit_engine(manifest_dir, native_dir);
    for path in topology_files {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    for path in chip_ball_files {
        println!("cargo:rerun-if-changed={}", path.display());
    }
}
