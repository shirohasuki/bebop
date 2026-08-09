use assert_cmd::Command;
use std::path::Path;
use std::time::Instant;

use super::super::artifacts::{ArtifactManager, RegressionResult, TestStatus};
use super::backend::BackendRunner;

pub fn run_backend_elf_test(
    bebop_bin: &Path,
    elf_path: &Path,
    verbose: bool,
    backend: &impl BackendRunner,
) -> RegressionResult {
    let workload_name = elf_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    let artifacts = match ArtifactManager::create_with_backend(backend.backend_name(), &workload_name) {
        Ok(a) => a,
        Err(e) => {
            return RegressionResult::infra_error(
                backend.backend_name(),
                &workload_name,
                elf_path,
                None,
                format!("Failed to create artifact directory: {}", e),
            );
        }
    };

    if verbose {
        eprintln!("Running {} for: {}", backend.verbose_run_kind(), elf_path.display());
        eprintln!("  Artifact dir: {}", artifacts.root().display());
    }

    let program = backend.command_program(bebop_bin, elf_path);
    let mut cmd = Command::new(&program);
    backend.configure_command_dir(&mut cmd, elf_path);
    backend.configure_command_env(&mut cmd, elf_path);
    backend.build_command(&mut cmd, bebop_bin, elf_path, &artifacts);
    cmd.timeout(backend.timeout());

    let start = Instant::now();
    let output = cmd.output();
    let elapsed = start.elapsed();
    let elapsed_sec = format!("{:.2}s", elapsed.as_secs_f64());

    let stdout_path = artifacts.stdout_path();
    let stderr_path = artifacts.stderr_path();
    let fst_path = if backend.needs_wave() {
        Some(artifacts.fst_waveform_path())
    } else {
        None
    };
    let log_path = if backend.needs_log_dir() {
        Some(artifacts.log_dir())
    } else {
        None
    };

    let result = match output {
        Ok(output) => {
            let stdout_str = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr_str = String::from_utf8_lossy(&output.stderr).to_string();

            let _ = artifacts.write_stdout(&stdout_str);
            let _ = artifacts.write_stderr(&stderr_str);

            let status = if output.status.success() {
                TestStatus::Pass
            } else if output.status.code().is_none() {
                TestStatus::Crash
            } else {
                TestStatus::Fail
            };

            RegressionResult {
                backend: backend.backend_name().to_string(),
                workload_name,
                elf_path: elf_path.to_path_buf(),
                elapsed_sec,
                status,
                exit_code: output.status.code(),
                stdout_path: Some(stdout_path),
                stderr_path: Some(stderr_path),
                artifact_dir: Some(artifacts.root().to_path_buf()),
                fst_path,
                log_path,
                error_message: None,
            }
        }
        Err(e) => {
            let status = if e.kind() == std::io::ErrorKind::TimedOut {
                TestStatus::Timeout
            } else {
                TestStatus::InfraError
            };

            let error_message = format!("{}", e);

            if let Some(ref log_dir) = log_path {
                let sim_stdout = log_dir.join("stdout.log");
                if sim_stdout.exists() {
                    let _ = std::fs::copy(&sim_stdout, &stdout_path);
                }
                let sim_stderr = log_dir.join("stderr.log");
                if sim_stderr.exists() {
                    let _ = std::fs::copy(&sim_stderr, &stderr_path);
                }
            }

            RegressionResult {
                backend: backend.backend_name().to_string(),
                workload_name,
                elf_path: elf_path.to_path_buf(),
                elapsed_sec,
                status,
                exit_code: None,
                stdout_path: Some(stdout_path),
                stderr_path: Some(stderr_path),
                artifact_dir: Some(artifacts.root().to_path_buf()),
                fst_path,
                log_path,
                error_message: Some(error_message),
            }
        }
    };

    let _ = artifacts.write_summary(&result);

    let _preserved = artifacts.finalize(result.success());

    result
}

pub fn print_failure_details(result: &RegressionResult, test_name: &str) {
    eprintln!("\n=== Test Failed: {} ===", test_name);
    eprintln!("Status: {}", result.status);
    eprintln!("Backend: {}", result.backend);
    eprintln!("Elapsed: {}", result.elapsed_sec);

    if let Some(ref msg) = result.error_message {
        eprintln!("Error: {}", msg);
    }

    if let Some(code) = result.exit_code {
        eprintln!("Exit code: {}", code);
    }

    if let Some(ref dir) = result.artifact_dir {
        eprintln!("Artifact dir: {}", dir.display());
    }

    if let Some(ref p) = result.stderr_path {
        if p.exists() {
            if let Ok(content) = std::fs::read_to_string(p) {
                if !content.is_empty() {
                    eprintln!("\n--- stderr ---");
                    eprintln!("{}", content);
                }
            }
        }
    }

    if let Some(ref p) = result.stdout_path {
        if p.exists() {
            if let Ok(content) = std::fs::read_to_string(p) {
                if !content.is_empty() {
                    eprintln!("\n--- stdout ---");
                    eprintln!("{}", content);
                }
            }
        }
    }

    eprintln!("==================\n");
}
