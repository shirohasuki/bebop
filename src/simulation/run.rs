//===--- run.rs ----- simulation run entry point ------------------------===//
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
// Why we implement this module here instead of in different modules for each target?
//   In simulation directory we have a higher level of different tools. In order to
//   better manage the co-simulation between different tools, we place targets entries
//   in this module.
//
// In bebop, a simulation target is exclusively linked to a specific tool, because
// we believe that in current RTL simulation, one tool is bound to dominate. We
// consider that giving multiple tools equal standing in every scenario would
// result in excessive encapsulation.
//
//===----------------------------------------------------------------------===//

use crate::{RunCommand, RunTarget};
use snafu::Whatever;

pub fn run(command: RunCommand) -> Result<(), Whatever> {
    match command.target {
        RunTarget::Verilator {
            elf,
            log_dir,
            no_wave,
            diff,
            fast,
            itrace,
            mtrace,
            pmctrace,
            ctrace,
            banktrace,
        } => {
            #[cfg(feature = "verilator")]
            {
                crate::simulation::verilator::run::run(crate::simulation::verilator::run::VerilatorRunConfig {
                    elf,
                    log_dir,
                    wave: !no_wave,
                    diff,
                    fast,
                    trace: crate::simulation::verilator::run::VerilatorTraceConfig {
                        itrace,
                        mtrace,
                        pmctrace,
                        ctrace,
                        banktrace,
                    },
                })
            }
            #[cfg(not(feature = "verilator"))]
            {
                let _ = (
                    elf, log_dir, no_wave, diff, fast, itrace, mtrace, pmctrace, ctrace, banktrace,
                );
                crate::simulation::verilator::run::run_unavailable()
            }
        }
        RunTarget::Bemu {
            elf,
            log_dir,
            pk,
            bank_digest,
            disasm,
            tool_profile,
        } => crate::simulation::bemu::run::run(crate::simulation::bemu::run::BemuRunConfig {
            elf,
            log_dir,
            pk,
            bank_digest,
            disasm,
            tool_profile,
        }),
        RunTarget::P2e {
            image,
            bitstream,
            log_dir,
            multi_fpga,
            wave,
            wave_start,
            itrace,
            mtrace,
            pmctrace,
            ctrace,
            banktrace,
        } => {
            #[cfg(feature = "p2e")]
            {
                crate::simulation::p2e::run::run(crate::simulation::p2e::run::P2eRunConfig {
                    image,
                    bitstream,
                    log_dir,
                    multi_fpga,
                    wave,
                    wave_start,
                    trace: crate::simulation::p2e::run::P2eTraceConfig {
                        itrace,
                        mtrace,
                        pmctrace,
                        ctrace,
                        banktrace,
                    },
                })
            }
            #[cfg(not(feature = "p2e"))]
            {
                let _ = (
                    image, bitstream, log_dir, multi_fpga, wave, wave_start, itrace, mtrace, pmctrace, ctrace,
                    banktrace,
                );
                crate::simulation::p2e::run::run_unavailable()
            }
        }
    }
}
