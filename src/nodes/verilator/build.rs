//===- build.rs - Build Bebop Verilator for RTL simulation -----------------===//
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
//===---------------------------------------------------------------------------===//
//
//
//
//===---------------------------------------------------------------------------===//

use std::collections::HashSet;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const TOPNAME: &str = "BBSimHarness";

const VERILATOR_ARGS: &[&str] = &[
    "-MMD",
    "-cc",
    "--vpi",
    "--trace",
    "-O3",
    "--x-assign",
    "fast",
    "--x-initial",
    "fast",
    "--noassert",
    "-Wno-fatal",
    "--trace-fst",
    "--trace-threads",
    "1",
    "--output-split",
    "10000",
    "--output-split-cfuncs",
    "100",
    "--unroll-count",
    "256",
    "-Wall",
    "-Wno-PINCONNECTEMPTY",
    "-Wno-ASSIGNDLY",
    "-Wno-DECLFILENAME",
    "-Wno-UNUSED",
    "-Wno-UNUSEDSIGNAL",
    "-Wno-UNOPTFLAT",
    "-Wno-BLKANDNBLK",
    "-Wno-style",
    "--timing",
];

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let native_dir = manifest_dir.join("native");
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let obj_dir = out_dir.join("obj_dir");

    let build_dir = resolve_vsrc_path();
    let jobs = capped_jobs();

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={}", build_dir.display());
    println!("cargo:rerun-if-env-changed=VSRC_PATH");
    println!("cargo:rerun-if-env-changed=RISCV");

    let riscv = require_nix_riscv();

    let vsrcs = collect_files(&build_dir, &["v", "sv"]);
    let csrcs = collect_build_csrcs(&build_dir);
    let native_inputs = collect_files(&native_dir, &["c", "cc", "cpp", "h", "hh", "hpp"]);
    for src in vsrcs.iter().chain(csrcs.iter()).chain(native_inputs.iter()) {
        println!("cargo:rerun-if-changed={}", src.display());
    }

    if obj_dir.exists() {
        fs::remove_dir_all(&obj_dir).expect("remove stale obj_dir");
    }
    fs::create_dir_all(&obj_dir).expect("create obj_dir");
    run_verilator(&build_dir, &obj_dir, TOPNAME, &jobs, &vsrcs, &csrcs);

    let verilator_root = get_verilator_root(&obj_dir, TOPNAME);
    let generated_cpps = collect_verilator_cpps(&obj_dir);

    let mut build = cc::Build::new();
    build.compiler(require_gxx());
    build.cpp(true);
    build.std("c++17");
    build.warnings(false);
    build.opt_level(3);
    build.out_dir(&out_dir);
    build.cargo_metadata(false);
    build.flag_if_supported("-fcoroutines");
    build.flag_if_supported("-faligned-new");
    build.flag_if_supported("-fcf-protection=none");
    build.flag_if_supported("-pthread");

    build.define("VM_SC", "0");
    build.define("VM_TRACE", "1");
    build.define("VM_TRACE_FST", "1");
    build.define("VM_TRACE_VCD", "0");
    build.define("VM_TIMING", "1");

    build.include(&native_dir);
    build.include(native_dir.join("include"));
    build.include(&build_dir);
    build.include(&obj_dir);
    build.include(verilator_root.join("include"));
    build.include(verilator_root.join("include/vltstd"));
    build.include(&riscv.include_dir);

    // Compile minimal wrapper + memory model + generated Verilator code.
    // DPI-C trace callbacks are provided by bebop-rtl-trace.
    let native_csrcs = [
        native_dir.join("verilator.cc"),
        native_dir.join("memory/BBSimDRAM.cc"),
        native_dir.join("memory/mm.cc"),
        native_dir.join("memory/mm_dramsim3.cc"),
    ];
    for src in native_csrcs {
        build.file(src);
    }

    for file in &generated_cpps {
        build.file(file);
    }
    for support in verilator_support_sources(&verilator_root) {
        build.file(support);
    }

    // Add Verilator timing support library (for coroutines)
    build.file(verilator_root.join("include/verilated_timing.cpp"));

    build.compile("bebop_verilator_native");

    emit_link_config(&out_dir, &riscv);
}

struct NixRiscv {
    include_dir: PathBuf,
    lib_dir: PathBuf,
}

fn require_gxx() -> String {
    let cxx = "g++";
    let status = Command::new(cxx)
        .arg("--version")
        .stdout(Stdio::null())
        .status()
        .expect("g++ must be available in the nix development environment");
    assert!(
        status.success(),
        "g++ must be runnable in the nix development environment"
    );
    cxx.to_string()
}

fn require_nix_riscv() -> NixRiscv {
    let root = PathBuf::from(env::var("RISCV").expect("RISCV must be set by the nix development environment"));
    assert_exists(&root, "RISCV path does not exist");

    let include_dir = root.join("include");
    let lib_dir = root.join("lib");
    let dramsim3_config = root
        .join("share")
        .join("dramsim3")
        .join("configs")
        .join("DDR3_1Gb_x8_1333.ini");
    let required_files = vec![
        include_dir.join("fesvr/memif.h"),
        include_dir.join("fesvr/elfloader.h"),
        include_dir.join("dramsim3.h"),
        lib_dir.join("libfesvr.a"),
        lib_dir.join("libdramsim3.so"),
        lib_dir.join("libz.so"),
        dramsim3_config.clone(),
    ];
    for path in &required_files {
        assert_exists(path, "nix RISCV dependency is missing");
    }

    NixRiscv { include_dir, lib_dir }
}

fn emit_link_config(native_lib_dir: &Path, riscv: &NixRiscv) {
    println!("cargo:rustc-link-search=native={}", native_lib_dir.display());
    println!("cargo:rustc-link-search=native={}", riscv.lib_dir.display());
    println!("cargo:rustc-link-lib=static=bebop_verilator_native");
    println!("cargo:rustc-link-lib=static=fesvr");
    println!("cargo:rustc-link-lib=stdc++");
    println!("cargo:rustc-link-lib=dylib=dramsim3");
    println!("cargo:rustc-link-lib=lz4");
    println!("cargo:rustc-link-lib=z");
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", riscv.lib_dir.display());
}

fn resolve_vsrc_path() -> PathBuf {
    let path = env::var("VSRC_PATH").expect("VSRC_PATH must be set before building bebop-verilator");
    assert!(!path.trim().is_empty(), "VSRC_PATH must not be empty");
    let path = PathBuf::from(path);
    assert_exists(&path, "VSRC_PATH does not point to a Verilog source directory");
    path
}

fn capped_jobs() -> String {
    let jobs = env::var("NUM_JOBS")
        .unwrap_or_else(|_| "1".to_string())
        .parse::<usize>()
        .expect("NUM_JOBS must be a positive integer");
    assert!(jobs > 0, "NUM_JOBS must be a positive integer");
    jobs.min(16).to_string()
}

fn assert_exists(path: &Path, message: &str) {
    assert!(path.exists(), "{message}: {}", path.display());
}

fn collect_build_csrcs(build_dir: &Path) -> Vec<PathBuf> {
    collect_files(build_dir, &["c", "cc", "cpp"])
        .into_iter()
        .filter(|path| !path.components().any(|c| c.as_os_str() == OsStr::new("obj_dir")))
        .collect()
}

fn collect_files(root: &Path, exts: &[&str]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_files_inner(root, exts, &mut files);
    files.sort();
    files
}

// Verilator's *_vm_classes*.cpp files aggregate generated implementation
// files with #include. Compile each aggregate, but exclude its included files
// from the direct list to avoid duplicate symbols at link time.
fn collect_verilator_cpps(obj_dir: &Path) -> Vec<PathBuf> {
    let generated_cpps = collect_files(obj_dir, &["cpp"]);
    let included_cpps = generated_cpps
        .iter()
        .filter(|path| is_verilator_class_aggregate(path))
        .flat_map(|path| verilator_aggregate_includes(path))
        .collect::<HashSet<_>>();

    generated_cpps
        .into_iter()
        .filter(|path| !included_cpps.contains(path))
        .collect()
}

fn is_verilator_class_aggregate(path: &Path) -> bool {
    path.file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| name.contains("_vm_classes"))
}

fn verilator_aggregate_includes(path: &Path) -> Vec<PathBuf> {
    let content =
        fs::read_to_string(path).unwrap_or_else(|e| panic!("read Verilator aggregate {} failed: {e}", path.display()));
    let parent = path.parent().expect("Verilator aggregate parent directory");

    content
        .lines()
        .filter_map(|line| {
            let name = line.trim().strip_prefix("#include \"")?.strip_suffix("\"")?;
            (Path::new(name).extension() == Some(OsStr::new("cpp"))).then(|| parent.join(name))
        })
        .collect()
}

fn collect_files_inner(root: &Path, exts: &[&str], out: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(root).unwrap_or_else(|e| panic!("read directory {} failed: {e}", root.display()));
    for entry in entries {
        let path = entry
            .unwrap_or_else(|e| panic!("read directory entry under {} failed: {e}", root.display()))
            .path();
        if path.is_dir() {
            collect_files_inner(&path, exts, out);
            continue;
        }
        let Some(ext) = path.extension().and_then(OsStr::to_str) else {
            continue;
        };
        if exts.iter().any(|candidate| candidate.eq_ignore_ascii_case(ext)) {
            out.push(path);
        }
    }
}

fn get_verilator_root(obj_dir: &Path, topname: &str) -> PathBuf {
    let mk = obj_dir.join(format!("V{topname}.mk"));
    let contents = fs::read_to_string(&mk).expect("read generated V*.mk");
    let line = contents
        .lines()
        .find(|line| line.starts_with("VERILATOR_ROOT = "))
        .expect("VERILATOR_ROOT line");
    PathBuf::from(line.trim_start_matches("VERILATOR_ROOT = ").trim())
}

fn verilator_support_sources(verilator_root: &Path) -> Vec<PathBuf> {
    let include = verilator_root.join("include");
    vec![
        include.join("verilated.cpp"),
        include.join("verilated_dpi.cpp"),
        include.join("verilated_vpi.cpp"),
        include.join("verilated_fst_c.cpp"),
        include.join("verilated_threads.cpp"),
    ]
}

fn run_verilator(build_dir: &Path, obj_dir: &Path, topname: &str, jobs: &str, vsrcs: &[PathBuf], csrcs: &[PathBuf]) {
    let mut cmd = Command::new("verilator");
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());

    for arg in VERILATOR_ARGS {
        cmd.arg(arg);
    }

    cmd.arg("-j")
        .arg(jobs)
        .arg(format!("+incdir+{}", build_dir.display()))
        .arg("--top")
        .arg(topname)
        .arg("--Mdir")
        .arg(obj_dir);

    for src in vsrcs {
        cmd.arg(src);
    }
    for src in csrcs {
        cmd.arg(src);
    }

    let status = cmd.status().expect("run verilator");
    if !status.success() {
        panic!("verilator failed with status {status}");
    }
}
