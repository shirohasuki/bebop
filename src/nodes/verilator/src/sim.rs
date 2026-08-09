use crate::ffi::*;
use crate::mmio;
use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

static SHOULD_EXIT: AtomicBool = AtomicBool::new(false);

pub fn setup_ctrlc_handler() {
    ctrlc::set_handler(move || {
        eprintln!("Simulation interrupted");
        SHOULD_EXIT.store(true, Ordering::SeqCst);
    })
    .expect("failed to set Ctrl-C handler");
}

pub fn should_exit() -> bool {
    SHOULD_EXIT.load(Ordering::SeqCst)
}

pub struct Simulator {
    context: *mut VerilatorContext,
    top: *mut VerilatorTop,
    trace: *mut VerilatorTrace,
}

impl Simulator {
    pub fn new(fst_path: Option<&Path>, args: &[String]) -> io::Result<Self> {
        unsafe {
            let context = verilator_context_new();
            if context.is_null() {
                return Err(io::Error::other("failed to create Verilator context"));
            }

            if let Err(e) = set_command_args(context, args) {
                verilator_context_free(context);
                return Err(e);
            }

            let top = verilator_top_new(context);
            if top.is_null() {
                verilator_context_free(context);
                return Err(io::Error::other("failed to create top module"));
            }

            let trace = match init_waveform(context, top, fst_path) {
                Ok(trace) => trace,
                Err(e) => {
                    verilator_top_free(top);
                    verilator_context_free(context);
                    return Err(e);
                }
            };

            let mut sim = Simulator { context, top, trace };

            sim.reset();
            Ok(sim)
        }
    }

    fn reset(&mut self) {
        unsafe {
            // Chipyard reset synchronizers need multiple source-clock edges
            // before their downstream domains can leave reset. A single edge
            // leaves host-driven Frontend state indeterminate because there is
            // no Rocket instruction stream to provide a later reset sequence.
            verilator_top_set_reset(self.top, 1);
            for _ in 0..5 {
                verilator_top_set_clock(self.top, 0);
                self.step_and_dump();
                verilator_top_set_clock(self.top, 1);
                self.step_and_dump();
            }

            verilator_top_set_reset(self.top, 0);
            verilator_top_set_clock(self.top, 0);
            self.step_and_dump();
            self.exec_once();
        }
    }

    fn step_and_dump(&mut self) {
        unsafe {
            verilator_top_eval(self.top);
            verilator_context_time_inc(self.context, 1);
            let time = verilator_context_time(self.context);
            if !self.trace.is_null() {
                verilator_trace_dump(self.trace, time);
            }
        }
    }

    pub fn exec_once(&mut self) -> bool {
        unsafe {
            verilator_top_set_clock(self.top, 1);
            verilator_top_eval(self.top);

            let should_exit = mmio::should_exit();

            verilator_context_time_inc(self.context, 1);
            let time = verilator_context_time(self.context);
            if !self.trace.is_null() {
                verilator_trace_dump(self.trace, time);
            }

            verilator_top_set_clock(self.top, 0);
            self.step_and_dump();

            should_exit
        }
    }

    pub fn finalize(&mut self) {
        unsafe {
            verilator_context_time_inc(self.context, 1);
            let time = verilator_context_time(self.context);
            if !self.trace.is_null() {
                verilator_trace_dump(self.trace, time);
                verilator_trace_close(self.trace);
            }
        }
    }

    pub(crate) fn context_for_host_rush(&self) -> *mut VerilatorContext {
        self.context
    }
}

unsafe fn set_command_args(context: *mut VerilatorContext, args: &[String]) -> io::Result<()> {
    let mut all_args = vec!["bebop-verilator".to_string()];
    all_args.extend_from_slice(args);

    let c_args: Vec<CString> = all_args
        .iter()
        .map(|arg| CString::new(arg.as_str()).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e)))
        .collect::<io::Result<_>>()?;
    let c_argv: Vec<*const i8> = c_args.iter().map(|arg| arg.as_ptr()).collect();

    verilator_context_command_args(context, c_argv.len() as i32, c_argv.as_ptr());
    Ok(())
}

unsafe fn init_waveform(
    context: *mut VerilatorContext,
    top: *mut VerilatorTop,
    fst_path: Option<&Path>,
) -> io::Result<*mut VerilatorTrace> {
    let Some(fst_path) = fst_path else {
        return Ok(std::ptr::null_mut());
    };

    let trace = verilator_trace_new();
    if trace.is_null() {
        return Err(io::Error::other("failed to create trace"));
    }

    verilator_context_trace_ever_on(context, true);
    verilator_top_trace(top, trace, 0);

    let fst_cstr =
        CString::new(fst_path.as_os_str().as_bytes()).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    if !verilator_trace_open(trace, fst_cstr.as_ptr()) {
        verilator_trace_free(trace);
        return Err(io::Error::other("failed to open FST file"));
    }

    println!("Waveform will be saved to: {}", fst_path.display());
    Ok(trace)
}

impl Drop for Simulator {
    fn drop(&mut self) {
        unsafe {
            if !self.top.is_null() {
                verilator_top_free(self.top);
            }
            if !self.trace.is_null() {
                verilator_trace_free(self.trace);
            }
            if !self.context.is_null() {
                verilator_context_free(self.context);
            }
        }
    }
}
