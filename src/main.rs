//===----------------------------------------------------------------------===//
//
// Copyright 2026 The Aerospace Corporation
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
//===----------------------------------------------------------------------===//
//
// Bebop CLI entry point.
// It dispatches the CLI parsing into two separate execution paths:
// - build: to build simulator artifacts (build)
// - simulation: to run workloads on simulator built artifacts (run)
//
//===----------------------------------------------------------------------===//

use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

mod simulation;

#[derive(Debug, Parser)]
#[command(name = "bebop", about = "Bebop CLI")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Build a simulator artifact.
    Build(BuildCommand),
    /// Run a workload on a built simulator artifact.
    Run(RunCommand),
}

#[derive(Debug, Args)]
pub struct BuildCommand {
    #[command(subcommand)]
    pub target: BuildTarget,
}

#[derive(Debug, Subcommand)]
pub enum BuildTarget {
    /// Build a Verilator-based simulator.
    Verilator {
        #[arg(long, value_name = "DIR")]
        rtl_dir: PathBuf,
        #[arg(long, value_name = "DIR")]
        out_dir: PathBuf,
        #[arg(long, help = "Build a Verilator+BEMU difftest executable")]
        diff: bool,
        #[arg(long, help = "Build a Verilator+BEMU difftest executable")]
        fast: bool,
    },
    /// Build a P2E simulator artifact.
    P2e {
        #[arg(long, value_name = "DIR")]
        rtl_dir: PathBuf,
        #[arg(long, value_name = "DIR")]
        out_dir: PathBuf,
    },
}

#[derive(Debug, Args)]
pub struct RunCommand {
    #[command(subcommand)]
    pub target: RunTarget,
}

#[derive(Debug, Subcommand)]
pub enum RunTarget {
    /// Run a workload on a Verilator-based simulator artifact.
    Verilator {
        #[arg(long, value_name = "ELF")]
        elf: PathBuf,
        #[arg(long, value_name = "DIR")]
        log_dir: PathBuf,
        #[arg(long, help = "Disable waveform dump")]
        no_wave: bool,
        #[arg(long, help = "Run with a BEMU difftest instance")]
        diff: bool,
        #[arg(long, help = "Run with a BEMU fast/difftest instance")]
        fast: bool,
        #[arg(long, help = "Enable RTL instruction trace")]
        itrace: bool,
        #[arg(long, help = "Enable RTL memory trace")]
        mtrace: bool,
        #[arg(long, help = "Enable RTL performance counter trace")]
        pmctrace: bool,
        #[arg(long, help = "Enable RTL cycle counter trace")]
        ctrace: bool,
        #[arg(long, help = "Enable RTL bank trace")]
        banktrace: bool,
    },
    /// Run a workload on BEMU.
    Bemu {
        #[arg(long, value_name = "ELF")]
        elf: PathBuf,
        #[arg(long, value_name = "DIR")]
        log_dir: PathBuf,
        #[arg(long, help = "Run with proxy kernel (Linux mode, starts in S-mode)")]
        pk: bool,
        #[arg(long, help = "Generate DiffTest-N BEMU Golden Records")]
        bank_digest: bool,
        // For MobileNetV3 on pebble with --pk, a run without disassembly took
        // 12m30.50s while a run with it exceeded 45m22s: at least 3.6x slower.
        #[arg(long, help = "Enable per-instruction disassembly logging")]
        disasm: bool,
        #[arg(long, help = "Print coarse host-time breakdown for BEMU execution")]
        tool_profile: bool,
    },
    /// Run a workload on a P2E simulator artifact.
    P2e {
        #[arg(long, value_name = "IMAGE")]
        image: PathBuf,
        #[arg(long, value_name = "BIT")]
        bitstream: PathBuf,
        #[arg(long, value_name = "DIR")]
        log_dir: PathBuf,
        #[arg(long, help = "Use multi-FPGA hw_server connection without a location selector")]
        multi_fpga: bool,
        #[arg(long, help = "Enable waveform dump")]
        wave: bool,
        #[arg(long, help = "Start waveform dump from this cycle")]
        wave_start: Option<u64>,
        #[arg(long, help = "Enable RTL instruction trace")]
        itrace: bool,
        #[arg(long, help = "Enable RTL memory trace")]
        mtrace: bool,
        #[arg(long, help = "Enable RTL performance counter trace")]
        pmctrace: bool,
        #[arg(long, help = "Enable RTL cycle counter trace")]
        ctrace: bool,
        #[arg(long, help = "Enable RTL bank trace")]
        banktrace: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Commands::Build(command) => simulation::build(command),
        Commands::Run(command) => simulation::run(command),
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
