use super::btrace;
use std::cell::Cell;
use std::fs::{File, OpenOptions};
use std::io;
use std::io::Write;
use std::path::Path;

thread_local! {
    static CURRENT_TRACE: Cell<*mut TraceState> = const { Cell::new(std::ptr::null_mut()) };
}

#[derive(Default)]
pub struct TraceState {
    pub(super) bdb_file: Option<File>,
    pub(super) itrace: bool,
    pub(super) mtrace: bool,
    pub(super) clk: u64,
    pub(super) btrace: btrace::BtraceState,
}

#[derive(Clone, Debug, Default)]
pub struct TraceConfig {
    pub itrace: bool,
    pub mtrace: bool,
    pub btrace: bool,
}

impl TraceConfig {
    pub fn new(itrace: bool, mtrace: bool) -> Self {
        Self {
            itrace,
            mtrace,
            btrace: false,
        }
    }
}

impl TraceState {
    pub fn new(log_dir: &Path, config: TraceConfig) -> io::Result<Self> {
        std::fs::create_dir_all(log_dir)?;
        let bdb_file = if config.itrace || config.mtrace {
            Some(
                OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(log_dir.join("bdb.ndjson"))?,
            )
        } else {
            None
        };
        Ok(Self {
            bdb_file,
            itrace: config.itrace,
            mtrace: config.mtrace,
            btrace: btrace::init(log_dir, config.btrace)?,
            clk: 0,
        })
    }

    pub fn set_bemu_clk(&mut self, clk: u64) {
        self.clk = clk;
    }

    pub fn bemu_clk(&self) -> u64 {
        self.clk
    }

    pub fn btrace_enabled(&self) -> bool {
        self.btrace.enabled()
    }

    pub(super) fn write_itrace(&mut self, json: &str) {
        if self.itrace {
            write_ndjson(&mut self.bdb_file, json);
        }
    }

    pub(super) fn write_mtrace(&mut self, json: &str) {
        if self.mtrace {
            write_ndjson(&mut self.bdb_file, json);
        }
    }
}

fn write_ndjson(file: &mut Option<File>, json: &str) {
    if let Some(file) = file.as_mut() {
        writeln!(file, "{}", json).unwrap_or_else(|e| {
            panic!("failed to write bemu ndjson trace: {e}");
        });
        file.flush().unwrap_or_else(|e| {
            panic!("failed to flush bemu ndjson trace: {e}");
        });
    }
}

pub unsafe fn with_trace_ptr<R>(trace: *mut TraceState, f: impl FnOnce() -> R) -> R {
    struct TraceGuard {
        previous: *mut TraceState,
    }

    impl Drop for TraceGuard {
        fn drop(&mut self) {
            CURRENT_TRACE.with(|current| current.set(self.previous));
        }
    }

    CURRENT_TRACE.with(|current| {
        let _guard = TraceGuard {
            previous: current.replace(trace),
        };
        f()
    })
}

pub(super) fn with_current_trace(f: impl FnOnce(&mut TraceState)) {
    CURRENT_TRACE.with(|current| {
        let trace = current.get();
        if !trace.is_null() {
            f(unsafe { &mut *trace });
        }
    });
}
