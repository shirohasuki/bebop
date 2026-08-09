use clap::Parser;
use std::process::ExitCode;

mod common;

use common::{run_elf_regression, P2eBackend, RegressionArgs};

fn main() -> ExitCode {
    let args = RegressionArgs::parse();
    let bitstream = args
        .p2e_bitstream()
        .expect("BEBOP_P2E_BITSTREAM env var is required for test_p2e");
    run_elf_regression(
        args,
        "test_p2e",
        |tc| format!("p2e::{}", tc.name),
        "Make sure to build with: cargo build --features p2e",
        P2eBackend::new(bitstream),
    )
}
