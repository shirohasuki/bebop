//===------ run.rs ---------- Verilator simulation runner ----------------===//
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
// M4 tracks concurrent writers per <InstID, LogicalBankID> and emits records
// only when instruction completion and issued==arrived establish a Bank-Stable
// Boundary. Fast/checkpoint-parallel execution remains a later milestone.
//
//===----------------------------------------------------------------------===//
use snafu::{FromString, Whatever};
#[cfg(feature = "verilator")]
use std::path::PathBuf;

#[cfg(feature = "verilator")]
use snafu::ResultExt;
#[cfg(feature = "verilator")]
use std::fs::File;
#[cfg(feature = "verilator")]
use std::io::{BufReader, BufWriter};
#[cfg(feature = "verilator")]
use std::os::fd::AsRawFd;
#[cfg(feature = "verilator")]
use std::path::Path;

#[cfg(feature = "verilator")]
use bebop_fd_redirect::FdRedirect;

#[cfg(feature = "verilator")]
use bebop_verilator::{
    exit_code, init_trace, setup_ctrlc_handler, should_exit, write_trace_summary, Simulator, TraceConfig,
};

#[cfg(all(feature = "verilator", feature = "bemu"))]
use bebop_bank_hash::{
    init_runtime_packet_channel, run_online_compare_with_summary, runtime_packet_status,
    shutdown_runtime_packet_channel, BankHashCompareSummary,
};
#[cfg(all(feature = "verilator", feature = "bemu"))]
use bebop_bemu::{BemuInstance, TraceConfig as BemuTraceConfig};
#[cfg(all(feature = "verilator", feature = "bemu"))]
use bebop_verilator::{finish_bank_digest, poll_bank_digest, BankDigestConfig};
#[cfg(all(feature = "verilator", feature = "bemu"))]
use std::thread::JoinHandle;

#[cfg(feature = "verilator")]
use super::console::ConsoleServer;

#[cfg(feature = "verilator")]
pub struct VerilatorRunConfig {
    pub elf: PathBuf,
    pub log_dir: PathBuf,
    pub wave: bool,
    pub diff: bool,
    pub fast: bool,
    pub trace: VerilatorTraceConfig,
}

#[derive(Debug)]
#[cfg(feature = "verilator")]
pub struct VerilatorTraceConfig {
    pub itrace: bool,
    pub mtrace: bool,
    pub pmctrace: bool,
    pub ctrace: bool,
    pub banktrace: bool,
}

#[cfg(feature = "verilator")]
impl VerilatorRunConfig {
    fn mode(&self) -> &'static str {
        if self.fast && self.diff {
            "fast+diff"
        } else if self.fast {
            "fast"
        } else if self.diff {
            "diff"
        } else {
            "run"
        }
    }
}

#[cfg(feature = "verilator")]
pub fn run(config: VerilatorRunConfig) -> Result<(), Whatever> {
    //===----------------------------------------------------------------------===//
    // Configuration Checks
    //===----------------------------------------------------------------------===//
    setup_ctrlc_handler();
    if config.fast {
        return Err(Whatever::without_source(
            "Verilator fast run is not supported yet".to_string(),
        ));
    }
    #[cfg(not(feature = "bemu"))]
    if config.diff {
        return Err(Whatever::without_source(
            "this executable was built without BEMU; rebuild Verilator with --diff".to_string(),
        ));
    }

    let stdout_file = config.log_dir.join("stdout.log");
    let stderr_file = config.log_dir.join("stderr.log");
    let fst_file = config.log_dir.join("waveform").join("waveform.fst");

    #[cfg(feature = "bemu")]
    let bank_digest = config.diff.then(|| {
        let (bank_size, row_bytes) = bebop_bemu::private_bank_geometry();
        BankDigestConfig::new(bank_size, row_bytes)
    });

    #[cfg(not(feature = "bemu"))]
    let bank_digest = None;
    let trace_config = TraceConfig {
        itrace: config.trace.itrace,
        mtrace: config.trace.mtrace,
        pmctrace: config.trace.pmctrace,
        ctrace: config.trace.ctrace,
        banktrace: config.trace.banktrace || config.diff,
        bank_digest,
    };

    println!("ELF file: {}", config.elf.display());
    println!("Simulator mode: {}", config.mode());
    println!("Trace configuration: {:?}", config.trace);
    println!("Log directory: {}", config.log_dir.display());
    if config.wave {
        println!("Waveform will be saved to: {}", fst_file.display());
    }

    create_output_dirs(&config.log_dir, config.wave.then_some(fst_file.as_path()))?;
    init_trace(&config.log_dir, trace_config)
        .map_err(|e| Whatever::without_source(format!("failed to init Verilator trace: {e}")))?;

    //===----------------------------------------------------------------------===//
    // Initialize Verilator
    //===----------------------------------------------------------------------===//
    let stdout_guard = FdRedirect::new_tee(std::io::stdout().as_raw_fd(), &stdout_file, "stdout")
        .whatever_context("failed to redirect stdout")?;
    let stderr_guard = FdRedirect::new(std::io::stderr().as_raw_fd(), &stderr_file, "stderr")
        .whatever_context("failed to redirect stderr")?;

    let console = ConsoleServer::start(&config.log_dir)?;
    println!("Console socket: {}", console.socket_path().display());
    println!("UART logs: {}", console.uart_log_dir().display());

    let simulator_args = vec![format!("+elf={}", config.elf.display())];
    let mut simulator = Simulator::new(config.wave.then_some(fst_file.as_path()), &simulator_args)
        .map_err(|e| Whatever::without_source(format!("failed to create Verilator simulator: {e}")))?;

    #[cfg(feature = "bemu")]
    let mut diff_session = config
        .diff
        .then(|| DiffSession::new(&config.elf, &config.log_dir))
        .transpose()?;

    //===----------------------------------------------------------------------===//
    // Run
    //===----------------------------------------------------------------------===//
    loop {
        console.poll_tx();
        if simulator.exec_once() {
            break;
        }
        #[cfg(feature = "bemu")]
        if config.diff {
            poll_bank_digest().map_err(Whatever::without_source)?;
        }
        #[cfg(feature = "bemu")]
        if let Some(diff) = diff_session.as_mut() {
            diff.step_golden()?;
        }
        if should_exit() {
            break;
        }
    }
    console.poll_tx();
    let code = exit_code();

    //===----------------------------------------------------------------------===//
    // Finish Simulation
    //===----------------------------------------------------------------------===//
    simulator.finalize();

    #[cfg(feature = "bemu")]
    let diff_summary = if let Some(mut diff) = diff_session {
        finish_bank_digest().map_err(Whatever::without_source)?;
        diff.finish_golden()?;
        Some(diff.finish()?)
    } else {
        None
    };

    drop(console);
    drop(stderr_guard);
    drop(stdout_guard);

    write_trace_summary(&config.log_dir)
        .map_err(|error| Whatever::without_source(format!("failed to write RTL trace summary: {error}")))?;
    write_disasm_log(&stderr_file)?;

    #[cfg(feature = "bemu")]
    if let Some(summary) = diff_summary.as_ref() {
        println!(
            "Bank DiffTest M4 summary: pass={} mismatch={} missing_rtl={} unexpected_rtl={}",
            summary.pass, summary.mismatch, summary.missing_rtl, summary.unexpected_rtl
        );
    }
    if code != 0 {
        #[cfg(feature = "bemu")]
        if let Some(summary) = diff_summary.as_ref().filter(|summary| !summary.passed()) {
            return Err(Whatever::without_source(format!(
                "Verilator exited with code {code}; Bank DiffTest M4 failed: mismatch={} missing_rtl={} unexpected_rtl={}",
                summary.mismatch, summary.missing_rtl, summary.unexpected_rtl
            )));
        }
        return Err(Whatever::without_source(format!("Verilator exited with code {code}")));
    }
    #[cfg(feature = "bemu")]
    if let Some(summary) = diff_summary.filter(|summary| !summary.passed()) {
        return Err(Whatever::without_source(format!(
            "Bank DiffTest M4 failed: mismatch={} missing_rtl={} unexpected_rtl={}",
            summary.mismatch, summary.missing_rtl, summary.unexpected_rtl
        )));
    }
    Ok(())
}

#[cfg(all(feature = "verilator", feature = "bemu"))]
struct DiffSession {
    golden: BemuInstance,
    worker: Option<JoinHandle<Result<BankHashCompareSummary, String>>>,
}

#[cfg(all(feature = "verilator", feature = "bemu"))]
impl DiffSession {
    fn new(elf: &Path, log_dir: &Path) -> Result<Self, Whatever> {
        let receiver = init_runtime_packet_channel();
        let output = log_dir.join("bank_diff.ndjson");
        let worker = std::thread::Builder::new()
            .name("bank-diff-m4".to_string())
            .spawn(move || run_online_compare_with_summary(receiver, output).map_err(|error| error.to_string()))
            .map_err(|error| {
                shutdown_runtime_packet_channel();
                Whatever::without_source(format!("failed to start Bank DiffTest M4 worker: {error}"))
            })?;

        let golden_result = (|| {
            let golden_log_dir = log_dir.join("golden");
            let mut trace = BemuTraceConfig::new(false, false);
            trace.btrace = true;
            let mut golden = BemuInstance::new(&golden_log_dir, trace, false, false)
                .whatever_context("failed to create BEMU Golden Model")?;
            golden.load_elf(elf)?;
            golden.init_hart(false)?;
            Ok::<_, Whatever>(golden)
        })();
        let golden = match golden_result {
            Ok(golden) => golden,
            Err(error) => {
                shutdown_runtime_packet_channel();
                let _ = worker.join();
                return Err(error);
            }
        };

        Ok(Self {
            golden,
            worker: Some(worker),
        })
    }

    fn step_golden(&mut self) -> Result<(), Whatever> {
        if !self.golden.finished() {
            self.golden.step()?;
        }
        Ok(())
    }

    fn finish_golden(&mut self) -> Result<(), Whatever> {
        while !self.golden.finished() {
            self.golden.step()?;
        }
        let code = self.golden.exit_code().unwrap_or(0);
        if code != 0 {
            return Err(Whatever::without_source(format!(
                "BEMU Golden Model exited with code {code}"
            )));
        }
        Ok(())
    }

    fn finish(mut self) -> Result<BankHashCompareSummary, Whatever> {
        let packet_status = runtime_packet_status();
        shutdown_runtime_packet_channel();
        let summary = self
            .worker
            .take()
            .expect("DiffTest worker exists")
            .join()
            .map_err(|_| Whatever::without_source("Bank DiffTest M4 worker panicked".to_string()))?
            .map_err(Whatever::without_source)?;
        println!(
            "Bank DiffTest runtime packets: submitted={} no_sink={} send_failed={}",
            packet_status.submitted, packet_status.no_sink, packet_status.send_failed
        );
        Ok(summary)
    }
}

#[cfg(all(feature = "verilator", feature = "bemu"))]
impl Drop for DiffSession {
    fn drop(&mut self) {
        shutdown_runtime_packet_channel();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(not(feature = "verilator"))]
pub fn run_unavailable() -> Result<(), Whatever> {
    Err(Whatever::without_source(
        "verilator runner is not compiled into this executable".to_string(),
    ))
}

#[cfg(feature = "verilator")]
fn create_output_dirs(log_dir: &Path, fst_file: Option<&Path>) -> Result<(), Whatever> {
    std::fs::create_dir_all(log_dir).whatever_context("failed to create Verilator log dir")?;
    if let Some(parent) = fst_file.and_then(Path::parent) {
        std::fs::create_dir_all(parent).whatever_context("failed to create Verilator fst dir")?;
    }
    Ok(())
}

#[cfg(feature = "verilator")]
fn write_disasm_log(stderr_file: &Path) -> Result<(), Whatever> {
    let disasm_file = stderr_file.with_file_name("disasm.log");
    let input = File::open(stderr_file)
        .map_err(|e| Whatever::without_source(format!("failed to open stderr.log for disasm: {e}")))?;
    let output = File::create(&disasm_file)
        .map_err(|e| Whatever::without_source(format!("failed to create disasm.log: {e}")))?;

    if let Err(e) = bebop_dasm::process_dasm(BufReader::new(input), BufWriter::new(output)) {
        eprintln!("Warning: failed to disassemble Verilator stderr: {e}");
    } else {
        println!("Disassembly saved to: {}", disasm_file.display());
    }
    Ok(())
}
