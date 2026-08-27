use duct::cmd;
use std::fs;
use std::path::Path;

pub fn run_vvac(out_dir: &Path, sourceme: &Path, flist: &Path, top: &str) {
    let vvac_cmd = format!(
        r#"clang_format_bin="$(command -v clang-format || true)"
clang_format_bin="${{clang_format_bin%/*}}"
source {sourceme}
if [ -z "$HPEC_HOME" ]; then
    echo "HPEC_HOME not set after sourceme" >&2
    exit 1
fi
cc_bin="$HPEC_HOME/tools/gcc-8.3.0/gcc-8.3.0/bin/gcc"
cxx_bin="$HPEC_HOME/tools/gcc-8.3.0/gcc-8.3.0/bin/g++"
if [ ! -x "$cc_bin" ] || [ ! -x "$cxx_bin" ]; then
    echo "HPE gcc not found: $cc_bin $cxx_bin" >&2
    exit 1
fi
vvac_bin="$(command -v vvac || true)"
vvac_bin="${{vvac_bin%/*}}"
filtered_path=()
IFS=: read -r -a path_entries <<< "$PATH"
for path_entry in "${{path_entries[@]}}"; do
    case "$path_entry" in
        "$clang_format_bin"|"$vvac_bin"|/home/wanghui/Code/buckyball/result/bin|/usr/*|/bin|/sbin) ;;
        *) filtered_path+=("$path_entry") ;;
    esac
done
PATH="$(IFS=:; printf '%s' "${{filtered_path[*]}}")"
PATH="${{vvac_bin}}:$PATH"
export PATH
unset CMAKE_C_COMPILER CMAKE_CXX_COMPILER NIX_CC
export CC="$cc_bin"
export CXX="$cxx_bin"
vvac -bc -f {flist} -top {top}"#,
        sourceme = sourceme.display(),
        flist = flist.display(),
        top = top,
    );

    cmd!("bash", "-c", &vvac_cmd)
        .dir(out_dir)
        .stdout_to_stderr()
        .run()
        .unwrap_or_else(|e| {
            panic!(
                "vvac failed: {}. Check log: {}",
                e,
                out_dir.join("vvac_build.log").display()
            )
        });
}

pub fn add_missing_empty_modules(out_dir: &Path) -> bool {
    let filelist_path = out_dir.join("vvacDir/vvac_by_mod/filelist");
    if !filelist_path.exists() {
        println!("cargo:warning=VVAC filelist not found, skipping");
        return false;
    }

    let content = fs::read_to_string(&filelist_path).expect("Failed to read VVAC filelist");
    let vvac_dir = out_dir.join("vvacDir/vvac_by_mod");

    let empty_modules = [
        "work_DebugCustomXbar.sv",
        "work_IntSyncCrossingSource_n1x1_Registered.sv",
        "work_NullIntSource.sv",
        "work_SourceX.sv",
        "work_Queue1_SourceXRequest.sv",
    ];

    let mut added_count = 0;
    let mut new_lines = Vec::new();

    for module in &empty_modules {
        let file_path = vvac_dir.join(module);

        if file_path.exists() && !content.contains(module) {
            new_lines.push(format!("./{}", module));
            println!("cargo:warning=Adding missing empty module to filelist: {}", module);
            added_count += 1;
        }
    }

    if added_count == 0 {
        println!("cargo:warning=No missing empty modules found");
        return false;
    }

    let mut new_content = content;
    if !new_content.ends_with('\n') {
        new_content.push('\n');
    }
    new_content.push_str(&new_lines.join("\n"));
    new_content.push('\n');

    fs::write(&filelist_path, new_content).expect("Failed to write updated VVAC filelist");
    println!("cargo:warning=Added {} empty modules to VVAC filelist", added_count);
    true
}

pub fn remove_empty_module_instantiations(build_dir: &Path) {
    let empty_modules = [
        "IntSyncCrossingSource_n1x1_Registered",
        "NullIntSource",
        "IntXbar_i0_o0",
        "SourceX",
        "Queue1_SourceXRequest",
    ];

    let mut total_removed = 0;
    let entries = fs::read_dir(build_dir).expect("Failed to read Verilog source directory");
    for entry in entries {
        let path = entry.expect("Failed to read Verilog source entry").path();
        let ext = path.extension().and_then(|s| s.to_str());
        if ext != Some("v") && ext != Some("sv") {
            continue;
        }

        let content = fs::read_to_string(&path).expect("Failed to read Verilog source");
        let mut removed_count = 0;
        let new_content: String = content
            .lines()
            .filter(|line| {
                let trimmed = line.trim();
                let should_remove = empty_modules
                    .iter()
                    .any(|module| trimmed.starts_with(&format!("{module} ")) && trimmed.ends_with("();"));

                if should_remove {
                    println!("cargo:warning=Removing empty module instantiation: {}", trimmed);
                    removed_count += 1;
                }
                !should_remove
            })
            .collect::<Vec<_>>()
            .join("\n");

        if removed_count > 0 {
            fs::write(&path, new_content).expect("Failed to write updated Verilog source");
            println!(
                "cargo:warning=Removed {} empty module instantiations from {}",
                removed_count,
                path.display()
            );
            total_removed += removed_count;
        }
    }

    if total_removed == 0 {
        println!("cargo:warning=No empty module instantiations found in Verilog sources");
    } else {
        println!(
            "cargo:warning=Removed {} empty module instantiations from Verilog sources",
            total_removed
        );
    }
}

pub fn fix_vvac_library_rpath(out_dir: &Path) {
    // Why: VVAC libraries (libtbppeer.so, etc.) were compiled with libstdc++.so.6.0.25
    // but their RPATH points to non-existent build-time paths. Set RPATH to $ORIGIN
    // so they load libstdc++ from their own directory.

    let vvac_lib_dir = out_dir.join("vvacDir/runtimeDir/lib/lib_arm");
    if !vvac_lib_dir.exists() {
        println!("cargo:warning=VVAC lib directory not found, skipping RPATH fix");
        return;
    }

    let libraries = ["libtbppeer.so", "libvCtb.so", "libvmri.so"];

    for lib_name in &libraries {
        let lib_path = vvac_lib_dir.join(lib_name);
        if !lib_path.exists() {
            println!("cargo:warning={} not found, skipping", lib_name);
            continue;
        }

        println!("cargo:warning=Fixing RPATH for {}", lib_name);

        let result = cmd!("patchelf", "--set-rpath", "$ORIGIN", lib_path.to_str().unwrap()).run();

        match result {
            Ok(_) => println!("cargo:warning=Successfully fixed RPATH for {}", lib_name),
            Err(e) => println!("cargo:warning=Failed to fix RPATH for {}: {}", lib_name, e),
        }
    }
}
