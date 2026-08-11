use assert_cmd::Command;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::super::artifacts::ArtifactManager;
use super::super::discovery::ElfTestCase;

fn path_stem(path: &Path) -> Option<String> {
    path.file_name().map(|n| n.to_string_lossy().into_owned())
}

fn is_rushb_bemu(path: &Path) -> bool {
    path_stem(path).is_some_and(|n| n.ends_with("-rushB-bemu-run"))
}

fn is_rushb_verilator(path: &Path) -> bool {
    path_stem(path).is_some_and(|n| n.ends_with("-rushB-verilator-run"))
}

pub trait BackendRunner {
    fn backend_name(&self) -> &'static str;

    fn verbose_run_kind(&self) -> &'static str {
        "test"
    }

    fn timeout(&self) -> Duration;

    fn configure_command_env(&self, _cmd: &mut Command, _elf_path: &Path) {}

    fn build_command(&self, cmd: &mut Command, bebop_bin: &Path, elf_path: &Path, artifacts: &ArtifactManager);

    /// Guest backends use bebop; rushB host runners are executed directly.
    fn command_program(&self, bebop_bin: &Path, elf_path: &Path) -> PathBuf {
        if is_rushb_bemu(elf_path) || is_rushb_verilator(elf_path) {
            elf_path.to_path_buf()
        } else {
            bebop_bin.to_path_buf()
        }
    }

    fn configure_command_dir(&self, cmd: &mut Command, elf_path: &Path) {
        if is_rushb_bemu(elf_path) || is_rushb_verilator(elf_path) {
            if let Some(dir) = elf_path.parent() {
                cmd.current_dir(dir);
            }
        }
    }

    fn scan_extension(&self) -> Option<&'static str> {
        None
    }

    fn match_case(&self, test_case: &ElfTestCase) -> bool;

    fn needs_log_dir(&self) -> bool {
        false
    }

    fn needs_wave(&self) -> bool {
        false
    }
}

#[cfg(feature = "bemu")]
#[derive(Clone, Copy, Debug, Default)]
#[allow(dead_code)]
pub struct BemuBackend;

#[cfg(feature = "bemu")]
impl BackendRunner for BemuBackend {
    fn backend_name(&self) -> &'static str {
        "bemu"
    }

    fn build_command(&self, cmd: &mut Command, _bebop_bin: &Path, elf_path: &Path, artifacts: &ArtifactManager) {
        if is_rushb_bemu(elf_path) {
            return;
        }
        if is_rushb_verilator(elf_path) {
            panic!("bemu harness got verilator rushB runner: {}", elf_path.display());
        }

        cmd.arg("run");
        cmd.arg("bemu");
        cmd.arg("--elf").arg(elf_path);
        cmd.arg("--log-dir").arg(artifacts.log_dir());

        if let Some(stem) = elf_path.file_stem() {
            if stem.to_string_lossy().ends_with("-linux") {
                cmd.arg("--pk");
            }
        }
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(300)
    }

    fn match_case(&self, test_case: &ElfTestCase) -> bool {
        test_case.stem.ends_with("-rushB-bemu-run")
            || test_case.stem.ends_with("singlecore-baremetal")
            || test_case.stem.ends_with("-linux")
    }

    fn needs_log_dir(&self) -> bool {
        true
    }
}

#[cfg(feature = "verilator")]
#[derive(Clone, Copy, Debug, Default)]
#[allow(dead_code)]
pub struct VerilatorBackend {
    diff: bool,
}

#[cfg(feature = "verilator")]
impl VerilatorBackend {
    pub fn new(diff: bool) -> Self {
        Self { diff }
    }
}

#[cfg(feature = "verilator")]
impl BackendRunner for VerilatorBackend {
    fn backend_name(&self) -> &'static str {
        if self.diff {
            "difftest"
        } else {
            "verilator"
        }
    }

    fn verbose_run_kind(&self) -> &'static str {
        "verilator test"
    }

    fn build_command(&self, cmd: &mut Command, _bebop_bin: &Path, elf_path: &Path, artifacts: &ArtifactManager) {
        if is_rushb_verilator(elf_path) {
            return;
        }
        if is_rushb_bemu(elf_path) {
            panic!("verilator harness got bemu rushB runner: {}", elf_path.display());
        }

        cmd.arg("run");
        cmd.arg("verilator");
        cmd.arg("--elf").arg(elf_path);
        cmd.arg("--log-dir").arg(artifacts.log_dir());
        cmd.arg("--no-wave");
        if self.diff {
            cmd.arg("--diff");
        }
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(1800)
    }

    fn match_case(&self, test_case: &ElfTestCase) -> bool {
        test_case.stem.ends_with("-rushB-verilator-run")
            || test_case.stem.ends_with("singlecore-baremetal")
            || test_case.stem.ends_with("_singlecore-baremetal")
            || test_case.stem.ends_with("-linux")
    }

    fn configure_command_env(&self, cmd: &mut Command, elf_path: &Path) {
        if is_rushb_verilator(elf_path) {
            return;
        }
        let arch_config = std::env::var_os("BEBOP_ARCH_CONFIG")
            .unwrap_or_else(|| "sims.verilator.BuckyballToyVerilatorConfig".into());
        cmd.env("ARCH_CONFIG", arch_config);
        if self.diff {
            if let Some(preload) = std::env::var_os("BEBOP_DIFF_LD_PRELOAD") {
                cmd.env("LD_PRELOAD", preload);
            }
        }
    }

    fn configure_command_dir(&self, cmd: &mut Command, elf_path: &Path) {
        if is_rushb_verilator(elf_path) {
            if let Some(dir) = elf_path.parent() {
                cmd.current_dir(dir);
            }
            return;
        }
        if self.diff {
            if let Some(dir) = std::env::var_os("BEBOP_DIFF_RUN_DIR") {
                cmd.current_dir(dir);
            }
        }
    }

    fn needs_log_dir(&self) -> bool {
        true
    }

    fn needs_wave(&self) -> bool {
        false
    }
}

#[cfg(feature = "p2e")]
#[derive(Clone, Debug)]
pub struct P2eBackend {
    bitstream: PathBuf,
}

#[cfg(feature = "p2e")]
impl P2eBackend {
    pub fn new(bitstream: PathBuf) -> Self {
        Self { bitstream }
    }
}

#[cfg(feature = "p2e")]
impl BackendRunner for P2eBackend {
    fn backend_name(&self) -> &'static str {
        "p2e"
    }

    fn verbose_run_kind(&self) -> &'static str {
        "p2e test"
    }

    fn build_command(&self, cmd: &mut Command, _bebop_bin: &Path, elf_path: &Path, artifacts: &ArtifactManager) {
        if is_rushb_bemu(elf_path) || is_rushb_verilator(elf_path) {
            panic!("p2e does not support rushB runners: {}", elf_path.display());
        }
        cmd.arg("run");
        cmd.arg("p2e");
        cmd.arg("--image").arg(elf_path);
        cmd.arg("--bitstream").arg(&self.bitstream);
        cmd.arg("--log-dir").arg(artifacts.log_dir());
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(1800)
    }

    fn scan_extension(&self) -> Option<&'static str> {
        Some("hex")
    }

    fn match_case(&self, test_case: &ElfTestCase) -> bool {
        test_case.stem.ends_with("singlecore-baremetal")
            || (test_case.stem.starts_with("fw_payload-") && test_case.stem.ends_with("-pk"))
    }

    fn needs_log_dir(&self) -> bool {
        true
    }
}
