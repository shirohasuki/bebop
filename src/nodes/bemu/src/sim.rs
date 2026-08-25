//===- sim.rs - BEMU simulation entry point -------------------------------===//
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
// BEMU (Buckyball Emulator) wraps Spike ISA simulator with custom RoCC
// instructions for Buckyball accelerator emulation.
//
//===----------------------------------------------------------------------===//

use snafu::{OptionExt, ResultExt, Whatever};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::{ffi::SharedMemory, spike::SpikeInstance, trace::TraceConfig};
use bebop_bemu_profile::BemuProfileReport;

pub struct BemuInstance {
    spike: SpikeInstance,
}

impl BemuInstance {
    pub fn new(log_dir: &Path, trace_config: TraceConfig, disasm: bool, profile: bool) -> Result<Self, Whatever> {
        crate::config::configure_default();
        Ok(Self {
            spike: SpikeInstance::new(log_dir, trace_config, disasm, profile, 0, None)
                .whatever_context("failed to create spike instance")?,
        })
    }

    /// Create a worker bound to one chip.pb core index. The caller must
    /// invoke this on the worker thread; configuration is deliberately thread-local.
    pub fn new_with_core(
        log_dir: &Path,
        trace_config: TraceConfig,
        disasm: bool,
        profile: bool,
        core_index: usize,
    ) -> Result<Self, Whatever> {
        Self::new_with_core_hart(log_dir, trace_config, disasm, profile, core_index, 0, None, None)
    }

    pub fn new_with_core_hart(
        log_dir: &Path,
        trace_config: TraceConfig,
        disasm: bool,
        profile: bool,
        core_index: usize,
        hart_id: usize,
        shared_memory: Option<Arc<SharedMemory>>,
        virtual_bank_count: Option<usize>,
    ) -> Result<Self, Whatever> {
        if let Some(virtual_bank_count) = virtual_bank_count {
            crate::config::configure_core_with_virtual_bank_count(core_index, virtual_bank_count);
        } else {
            crate::config::configure_core(core_index);
        }
        Ok(Self {
            spike: SpikeInstance::new(log_dir, trace_config, disasm, profile, hart_id, shared_memory)
                .whatever_context("failed to create spike instance")?,
        })
    }

    pub fn load_elf(&mut self, elf: &Path) -> Result<(), Whatever> {
        let elf = elf.to_str().whatever_context("invalid elf path")?;
        self.spike.load_elf(elf).whatever_context("failed to load bemu elf")
    }

    pub fn init_hart(&mut self, pk: bool) -> Result<(), Whatever> {
        self.spike
            .init_hart(pk)
            .whatever_context("failed to initialize bemu hart")
    }

    pub fn step(&mut self) -> Result<(), Whatever> {
        self.spike.step().whatever_context("bemu step failed")
    }

    pub fn barrier_hit(&self) -> bool {
        self.spike.barrier_hit()
    }

    pub fn stop(&mut self, code: i32) {
        self.spike.stop(code);
    }

    pub fn finished(&self) -> bool {
        self.spike.finished()
    }

    pub fn exit_code(&self) -> Option<i32> {
        self.spike.exit_code()
    }

    pub fn total_latency(&self) -> u64 {
        self.spike.total_latency()
    }

    pub fn profile_report(&self, total: Duration) -> Option<BemuProfileReport> {
        self.spike.profile_report(total)
    }
}
