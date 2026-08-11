use clap::Parser;
use std::process::ExitCode;

mod common;

use common::{run_elf_regression, RegressionArgs, VerilatorBackend};

fn main() -> ExitCode {
    let args = RegressionArgs::parse();
    let diff =
        std::env::var("BEBOP_VERILATOR_DIFF").is_ok_and(|value| matches!(value.as_str(), "1" | "true" | "yes" | "on"));
    let test_prefix = if diff { "difftest" } else { "verilator" };
    run_elf_regression(
        args,
        "test_verilator",
        move |tc| format!("{}::{}", test_prefix, tc.name),
        "Make sure to build with: cargo build --features verilator",
        VerilatorBackend::new(diff),
    )
}
