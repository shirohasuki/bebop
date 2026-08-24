// FFI bindings to minimal C++ Verilator wrapper

use std::os::raw::{c_char, c_int};

#[repr(C)]
pub struct VerilatorContext {
    _private: [u8; 0],
}

#[repr(C)]
pub struct VerilatorTop {
    _private: [u8; 0],
}

#[repr(C)]
pub struct VerilatorTrace {
    _private: [u8; 0],
}

extern "C" {
    // Verilator context management
    pub fn verilator_context_new() -> *mut VerilatorContext;
    pub fn verilator_context_free(ctx: *mut VerilatorContext);
    pub fn verilator_context_time_inc(ctx: *mut VerilatorContext, add: u64);
    pub fn verilator_context_time(ctx: *mut VerilatorContext) -> u64;
    pub fn verilator_context_command_args(ctx: *mut VerilatorContext, argc: c_int, argv: *const *const c_char);
    pub fn verilator_context_trace_ever_on(ctx: *mut VerilatorContext, on: bool);

    // Top module
    pub fn verilator_top_new(ctx: *mut VerilatorContext) -> *mut VerilatorTop;
    pub fn verilator_top_free(top: *mut VerilatorTop);
    pub fn verilator_top_eval(top: *mut VerilatorTop);
    pub fn verilator_top_trace(top: *mut VerilatorTop, tfp: *mut VerilatorTrace, levels: c_int);

    // Top module signals
    pub fn verilator_top_set_clock(top: *mut VerilatorTop, val: u8);
    pub fn verilator_top_set_reset(top: *mut VerilatorTop, val: u8);

    // rushB command bridge.
    pub fn verilator_rushb_clear();
    pub fn verilator_rushb_submit(accelerator_id: u32, tag: u64, xs1: u64, xs2: u64, funct7: u32);
    pub fn verilator_rushb_accepted(accelerator_id: u32) -> u64;
    pub fn verilator_rushb_complete_on_accept(accelerator_id: u32, tag: u64);
    pub fn verilator_rushb_completed(accelerator_id: u32) -> u64;
    pub fn verilator_rushb_inflight(accelerator_id: u32) -> u64;
    pub fn verilator_rushb_take_completed(accelerator_id: u32, tag: *mut u64) -> bool;
    pub fn verilator_rushb_probes(accelerator_id: u32) -> u64;
    pub fn verilator_rushb_last_ready(accelerator_id: u32) -> bool;
    pub fn verilator_rushb_last_retired(accelerator_id: u32) -> bool;

    // BBSimDRAM host staging API. These functions reject addresses outside
    // the physical backing region instead of exposing its raw mmap pointer.
    pub fn bbsim_host_memory_range(chip_id: i32, base: *mut u64, size: *mut u64) -> bool;
    pub fn bbsim_host_memory_write(chip_id: i32, address: u64, src: *const u8, size: u64) -> bool;
    pub fn bbsim_host_memory_read(chip_id: i32, address: u64, dst: *mut u8, size: u64) -> bool;

    // SCU state query (DPI-C functions are called from RTL automatically)
    pub fn verilator_scu_has_exit() -> bool;
    pub fn verilator_scu_exit_code() -> i32;
    pub fn verilator_scu_push_uart_rx(hart_id: u32, byte: u32);
    pub fn verilator_scu_drain_uart_tx(buf: *mut u32, len: u32) -> u32;

    // FST trace
    pub fn verilator_trace_new() -> *mut VerilatorTrace;
    pub fn verilator_trace_free(tfp: *mut VerilatorTrace);
    pub fn verilator_trace_open(tfp: *mut VerilatorTrace, filename: *const c_char) -> bool;
    pub fn verilator_trace_dump(tfp: *mut VerilatorTrace, timeui: u64);
    pub fn verilator_trace_close(tfp: *mut VerilatorTrace);
}
