use assert_cmd::Command;
use std::path::Path;
#[cfg(feature = "p2e")]
use std::path::PathBuf;
use std::time::Duration;

use super::super::artifacts::ArtifactManager;
use super::super::discovery::ElfTestCase;

pub trait BackendRunner {
    fn backend_name(&self) -> &'static str;

    fn verbose_run_kind(&self) -> &'static str {
        "test"
    }

    fn timeout(&self) -> Duration;

    fn configure_command_env(&self, _cmd: &mut Command) {}

    fn build_command(&self, cmd: &mut Command, bebop_bin: &Path, elf_path: &Path, artifacts: &ArtifactManager);

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
        cmd.arg("run");
        cmd.arg("bemu");
        cmd.arg("--elf").arg(elf_path);
        cmd.arg("--log-dir").arg(artifacts.log_dir());

        // Auto-detect pk mode: if filename ends with "-linux", use --pk
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
        test_case.stem.ends_with("singlecore-baremetal") || test_case.stem.ends_with("-linux")
    }

    fn needs_log_dir(&self) -> bool {
        true
    }
}

#[cfg(feature = "verilator")]
#[derive(Clone, Copy, Debug, Default)]
#[allow(dead_code)]
pub struct VerilatorBackend;

#[cfg(feature = "verilator")]
impl BackendRunner for VerilatorBackend {
    fn backend_name(&self) -> &'static str {
        "verilator"
    }

    fn verbose_run_kind(&self) -> &'static str {
        "verilator test"
    }

    fn build_command(&self, cmd: &mut Command, _bebop_bin: &Path, elf_path: &Path, artifacts: &ArtifactManager) {
        cmd.arg("run");
        cmd.arg("verilator");
        cmd.arg("--elf").arg(elf_path);
        cmd.arg("--log-dir").arg(artifacts.log_dir());
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(1800)
    }

    fn match_case(&self, test_case: &ElfTestCase) -> bool {
        test_case.stem.ends_with("singlecore-baremetal")
    }

    fn configure_command_env(&self, cmd: &mut Command) {
        cmd.env("ARCH_CONFIG", "sims.verilator.BuckyballToyVerilatorConfig");
    }

    fn needs_log_dir(&self) -> bool {
        true
    }

    fn needs_wave(&self) -> bool {
        true
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
