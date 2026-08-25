use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub fn native_dir(manifest_dir: &Path) -> PathBuf {
    let start = fs::canonicalize(manifest_dir).unwrap_or_else(|_| manifest_dir.to_path_buf());
    let local = start.join("native");
    if local.join("spike").exists() {
        return local;
    }

    for dir in start.ancestors() {
        let repo_native = dir.join("bebop").join("src").join("nodes").join("bemu").join("native");
        if repo_native.join("spike").exists() {
            return repo_native;
        }
    }

    panic!(
        "missing Spike at {}/native/spike (nix develop in bebop/ clones it)",
        start.display()
    );
}

pub fn build_and_link(native_dir: &Path, spike_dir: &Path, build_dir: &Path, install_dir: &Path) {
    if !spike_dir.exists() || !spike_dir.join("configure.ac").exists() {
        panic!("Spike missing at {}.", spike_dir.display());
    }

    build_spike(spike_dir, build_dir, install_dir);

    cc::Build::new()
        .cpp(true)
        .file(native_dir.join("spike.cc"))
        .file(native_dir.join("rocc.cc"))
        .file(native_dir.join("btif.cc"))
        .include(install_dir.join("include/riscv"))
        .include(install_dir.join("include/fesvr"))
        .flag("-std=c++17")
        .compile("spike_wrapper");

    println!("cargo:rustc-link-search=native={}/lib", install_dir.display());
    println!("cargo:rustc-link-lib=dylib=riscv");
    println!("cargo:rustc-link-lib=dylib=disasm");
    println!("cargo:rustc-link-lib=dylib=softfloat");
    println!("cargo:rustc-link-lib=dylib=fesvr");
    println!("cargo:rustc-link-lib=dylib=stdc++");
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}/lib", install_dir.display());
}

pub fn build_spike(spike_dir: &Path, build_dir: &Path, install_dir: &Path) {
    fs::create_dir_all(build_dir).expect("create spike build dir");
    fs::create_dir_all(install_dir).expect("create spike install dir");

    if !build_dir.join("Makefile").exists() {
        spike_configure(spike_dir, build_dir, install_dir);
    }

    let jobs = env::var("NUM_JOBS").unwrap_or_else(|_| "1".into());
    let st = Command::new("make")
        .current_dir(build_dir)
        .arg("-j")
        .arg(&jobs)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .expect("failed to execute make");
    if !st.success() {
        panic!("spike build failed with status {}", st);
    }

    let st = Command::new("make")
        .current_dir(build_dir)
        .arg("install")
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .expect("failed to execute make install");
    if !st.success() {
        panic!("spike install failed with status {}", st);
    }
}

fn spike_configure(spike_dir: &Path, build_dir: &Path, install_dir: &Path) {
    let st = Command::new(spike_dir.join("configure"))
        .current_dir(build_dir)
        .arg("--prefix")
        .arg(install_dir)
        .args(["--with-boost=no", "--with-boost-asio=no", "--with-boost-regex=no"])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .expect("failed to execute configure");
    if !st.success() {
        panic!("spike configure failed with status {}", st);
    }
}
