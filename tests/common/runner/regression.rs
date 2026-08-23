use libtest_mimic::{Arguments, Trial};
use std::path::PathBuf;
use std::process::ExitCode;

use super::super::args::RegressionArgs;
use super::super::artifacts::ArtifactManager;
use super::super::discovery::{discover_tests, write_nextest_terse_list, ElfTestCase};
use super::backend::BackendRunner;
use super::exec::{print_failure_details, run_backend_elf_test};

fn resolve_runner_bin() -> PathBuf {
    if let Some(path) = option_env!("CARGO_BIN_EXE_bebop") {
        return PathBuf::from(path);
    }
    if let Some(path) = option_env!("CARGO_BIN_EXE_bebop_bemu") {
        return PathBuf::from(path);
    }
    panic!("missing CARGO_BIN_EXE_bebop or CARGO_BIN_EXE_bebop_bemu for regression harness");
}

pub fn run_elf_regression<B, F>(
    args: RegressionArgs,
    harness_binary_name: &'static str,
    test_case_display_name: F,
    missing_bebop_hint: &'static str,
    backend: B,
) -> ExitCode
where
    B: BackendRunner + Clone + Send + Sync + 'static,
    F: Fn(&ElfTestCase) -> String + Clone + Send + Sync + 'static,
{
    if args.list {
        if args.format.as_deref() != Some("terse") {
            eprintln!("error: --list requires --format terse");
            return ExitCode::FAILURE;
        }
        let name_fn = test_case_display_name.clone();
        let extension = backend.scan_extension();
        let backend_for_list = backend.clone();
        if let Err(e) = write_nextest_terse_list(
            &args,
            extension,
            move |tc| backend_for_list.match_case(tc),
            move |tc| name_fn(tc),
        ) {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
        return ExitCode::SUCCESS;
    }

    let bebop_bin = resolve_runner_bin();

    if !bebop_bin.exists() {
        eprintln!("Error: bebop binary not found at: {}", bebop_bin.display());
        eprintln!("{missing_bebop_hint}");
        return ExitCode::FAILURE;
    }

    let extension = backend.scan_extension();
    let backend_for_discovery = backend.clone();
    let test_cases = match discover_tests(&args, extension, move |tc| backend_for_discovery.match_case(tc)) {
        Ok(cases) => cases,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::FAILURE;
        }
    };

    if test_cases.is_empty() {
        eprintln!("Warning: No test files found in: {}", args.bb_tests_root().display());
        return ExitCode::SUCCESS;
    }

    if args.clean_before {
        if let Err(e) = ArtifactManager::clean_all() {
            eprintln!("Error: failed to clean previous test artifacts: {e}");
            return ExitCode::FAILURE;
        }
    }

    if args.verbose {
        eprintln!("Found {} ELF test cases", test_cases.len());
        for tc in &test_cases {
            eprintln!("  - {}", tc.name);
        }
        eprintln!("Running {} tests after filtering", test_cases.len());
    }

    let trials: Vec<Trial> = test_cases
        .into_iter()
        .map(|test_case| {
            let name = test_case_display_name.clone()(&test_case);
            let bebop_bin = bebop_bin.clone();
            let elf_path = test_case.path.clone();
            let verbose = args.verbose;
            let backend = backend.clone();

            Trial::test(name, move || {
                let result = run_backend_elf_test(&bebop_bin, &elf_path, verbose, &backend);
                if result.success() {
                    Ok(())
                } else {
                    let test_name = elf_path.file_stem().unwrap_or_default().to_string_lossy();
                    print_failure_details(&result, &test_name);
                    Err(format!("Test failed: {}", elf_path.display()).into())
                }
            })
        })
        .collect();

    let mut libtest_args = vec![harness_binary_name.to_string()];
    libtest_args.extend(args.libtest_forward_flags());
    libtest_args.extend(args.test_args);
    let test_args = Arguments::from_iter(libtest_args);

    libtest_mimic::run(&test_args, trials).exit_code()
}
