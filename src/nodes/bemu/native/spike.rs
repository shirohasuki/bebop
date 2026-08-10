use crate::ffi::{create_spike, NativeSpike};
use crate::trace::TraceConfig;
use bebop_bemu_profile::BemuProfileReport;
use std::path::Path;
use std::sync::Arc;

use crate::ffi::SharedMemory;
use std::time::Duration;

pub struct SpikeInstance {
    mem_mb: usize,
    native: NativeSpike,
}

impl SpikeInstance {
    pub fn new(
        log_dir: &Path,
        trace_config: TraceConfig,
        disasm: bool,
        profile: bool,
        hart_id: usize,
        shared_memory: Option<Arc<SharedMemory>>,
    ) -> Result<Self, String> {
        let isa = "rv64gc_xbuckyball_zicclsm_zicntr_zihpm";
        let disasm_log_file = disasm.then(|| log_dir.join("disasm.log"));
        let disasm_log_file = disasm_log_file
            .as_deref()
            .map(|path| path.to_str().ok_or_else(|| "invalid log_dir path".to_string()))
            .transpose()?;
        let native = create_spike(isa, hart_id, shared_memory, disasm_log_file, log_dir, trace_config, profile)?;

        Ok(Self { mem_mb: 2048, native })
    }

    pub fn load_elf(&mut self, elf_file: &str) -> Result<(), String> {
        self.native.load_elf(elf_file)
    }

    pub fn init_hart(&mut self, pk: bool) -> Result<(), String> {
        self.native.init_hart(self.mem_mb, pk)
    }

    pub fn step(&mut self) -> Result<(), String> {
        self.native.step()
    }

    pub fn barrier_hit(&self) -> bool {
        self.native.barrier_hit()
    }

    pub fn finished(&self) -> bool {
        self.native.finished()
    }

    pub fn exit_code(&self) -> Option<i32> {
        Some(self.native.exit_code())
    }

    pub fn stop(&mut self, code: i32) {
        self.native.stop(code);
    }

    pub fn total_latency(&self) -> u64 {
        self.native.total_latency()
    }

    pub fn profile_report(&self, total: Duration) -> Option<BemuProfileReport> {
        self.native.profile_report(total)
    }
}
