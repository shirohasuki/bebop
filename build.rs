use std::env;
use std::path::PathBuf;

fn main() {
    if env::var("CARGO_FEATURE_VERILATOR").is_ok() {
        let riscv = env::var("RISCV").expect("RISCV must be set by the nix development environment");
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}/lib", riscv);
        println!("cargo:rustc-link-arg=-Wl,--enable-new-dtags");
    }

    if env::var("CARGO_FEATURE_P2E").is_ok() {
        let bebop_root = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
        let out_dir = env::var("OUT_PATH").unwrap_or_else(|_| format!("{bebop_root}/out"));
        let libvctb_dir = out_dir.clone();
        let vvac_lib_dir = format!("{out_dir}/vvacDir/runtimeDir/lib/lib_arm");
        println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN");
        println!("cargo:rustc-link-arg=-Wl,-rpath,{vvac_lib_dir}");
        println!("cargo:rustc-link-arg=-Wl,-rpath,{libvctb_dir}");
        println!("cargo:rustc-link-arg=-Wl,--enable-new-dtags");
    }
}
