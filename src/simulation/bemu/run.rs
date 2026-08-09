//===------ run.rs ---------- BEMU simulation runner --------------------===//
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
//
//
//===----------------------------------------------------------------------===//

use snafu::{FromString, Whatever};
use std::path::PathBuf;
use std::time::Instant;

#[cfg(feature = "bemu")]
use bebop_bemu::{format_profile_report, print_profile_report, BemuInstance, TraceConfig};

pub struct BemuRunConfig {
    pub elf: PathBuf,
    pub log_dir: PathBuf,
    pub pk: bool,
    pub bank_digest: bool,
    pub disasm: bool,
    pub tool_profile: bool,
}

pub fn run(config: BemuRunConfig) -> Result<(), Whatever> {
    #[cfg(feature = "bemu")]
    {
        let elf = config.elf.display();
        let log_dir = config.log_dir.display();
        println!("[INFO] Running BEMU: elf={elf} log_dir={log_dir}");

        // Step 1: Initialize BEMU
        let mut trace_config = TraceConfig::new(false, false);
        trace_config.btrace = config.bank_digest;

        let mut bemu = BemuInstance::new(&config.log_dir, trace_config, config.disasm, config.tool_profile)?;

        // Step 2: Load workload
        bemu.load_elf(&config.elf)?;

        // Step 3: Initialize hart
        bemu.init_hart(config.pk)?;

        // Step 4: Run bemu in a loop until finished
        let started = config.tool_profile.then(Instant::now);
        while !bemu.finished() {
            bemu.step()?;
        }
        if let Some(started) = started {
            if let Some(report) = bemu.profile_report(started.elapsed()) {
                print_profile_report(&report);
                let profile_path = config.log_dir.join("tool-profile.txt");
                std::fs::write(&profile_path, format_profile_report(&report)).map_err(|e| {
                    Whatever::without_source(format!("failed to write tool profile {}: {e}", profile_path.display()))
                })?;
            }
        }
        println!("[INFO] BEMU total latency: {}", bemu.total_latency());

        // Step 5: exit bemu simulation
        let exit_code = bemu.exit_code().unwrap_or(0);
        if exit_code != 0 {
            return Err(Whatever::without_source(format!("bemu exited with code {exit_code}")));
        }
        Ok(())
    }

    #[cfg(not(feature = "bemu"))]
    {
        let _ = config;
        let error_msg = "bemu is not enabled in this build".to_string();
        Err(Whatever::without_source(error_msg))
    }
}
