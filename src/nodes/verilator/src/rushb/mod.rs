mod async_mvout;
mod command;
mod dma;
mod scheduler;
mod state;

use bebop_rushb::{FUNCT7_MSET, FUNCT7_MVIN, FUNCT7_MVOUT};
use command::WaitMode;
use dma::DmaOperation;
use std::ffi::c_void;

#[no_mangle]
pub extern "C" fn rushb_init() {
    state::init();
}

#[no_mangle]
pub extern "C" fn rushb_destroy() {
    async_mvout::wait_all().unwrap_or_else(|error| panic!("rushB async mvout drain failed: {error}"));
    state::destroy();
}

#[no_mangle]
pub extern "C" fn rushb_select_accelerator(accelerator_id: u32, chip_id: i32) {
    state::select(accelerator_id, chip_id);
}

#[no_mangle]
pub extern "C" fn rushb_mset(xs1: u64, xs2: u64) {
    let selection = state::selection();
    command::execute(
        selection.accelerator_id,
        selection.chip_id,
        xs1,
        xs2,
        FUNCT7_MSET,
        WaitMode::Accepted,
        DmaOperation::None,
    )
    .unwrap_or_else(|error| panic!("rushB mset failed: {error}"));

    let bank_id = usize::try_from(xs1 & 0x3ff).expect("invalid bank id");
    let raw_cols = (xs2 >> 5) & 0x1f;
    let allocated = ((xs2 >> 10) & 1) != 0;
    // col=0 represents the elaborated accelerator's full bank width. The
    // generic host runtime cannot infer that width, so reject its DMA use.
    let groups = if allocated && raw_cols != 0 { raw_cols } else { 0 };
    state::update_bank_config(selection, bank_id, state::BankConfig { allocated, groups });
}

#[no_mangle]
pub extern "C" fn rushb_mvin(xs1: u64, packed_xs2: u64, host_ptr: *const c_void) {
    let selection = state::selection();
    let bank_id = usize::try_from(xs1 & 0x3ff).expect("invalid bank id");
    let spans = dma::spans(state::bank_config(selection, bank_id), xs1, packed_xs2);
    let chunks = unsafe { dma::capture_host(host_ptr.cast(), &spans) };
    command::execute(
        selection.accelerator_id,
        selection.chip_id,
        xs1,
        packed_xs2,
        FUNCT7_MVIN,
        WaitMode::Accepted,
        DmaOperation::Mvin { spans, chunks },
    )
    .unwrap_or_else(|error| panic!("rushB mvin failed: {error}"));
}

#[no_mangle]
pub extern "C" fn rushb_mvout(xs1: u64, packed_xs2: u64, host_ptr: *mut c_void) {
    let handle = submit_mvout(xs1, packed_xs2, host_ptr);
    async_mvout::wait(handle).unwrap_or_else(|error| panic!("rushB mvout failed: {error}"));
}

#[no_mangle]
pub extern "C" fn rushb_mvout_async(xs1: u64, packed_xs2: u64, host_ptr: *mut c_void) -> u64 {
    submit_mvout(xs1, packed_xs2, host_ptr)
}

#[no_mangle]
pub extern "C" fn rushb_mvout_poll(handle: u64) -> i32 {
    i32::from(async_mvout::poll(handle).unwrap_or_else(|error| panic!("rushB async mvout poll failed: {error}")))
}

#[no_mangle]
pub extern "C" fn rushb_mvout_wait(handle: u64) {
    async_mvout::wait(handle).unwrap_or_else(|error| panic!("rushB async mvout wait failed: {error}"));
}

fn submit_mvout(xs1: u64, packed_xs2: u64, host_ptr: *mut c_void) -> u64 {
    assert!(!host_ptr.is_null(), "mvout host pointer is null");
    let selection = state::selection();
    let bank_id = usize::try_from(xs1 & 0x3ff).expect("invalid bank id");
    let spans = dma::spans(state::bank_config(selection, bank_id), xs1, packed_xs2);
    let (handle, receiver) = command::submit(
        selection.accelerator_id,
        selection.chip_id,
        xs1,
        packed_xs2,
        FUNCT7_MVOUT,
        WaitMode::Completed,
        DmaOperation::Mvout { spans },
    )
    .unwrap_or_else(|error| panic!("rushB mvout submission failed: {error}"));
    async_mvout::register(handle, host_ptr.cast(), receiver)
        .unwrap_or_else(|error| panic!("rushB async mvout registration failed: {error}"));
    handle
}

#[no_mangle]
pub extern "C" fn rushb_custom(xs1: u64, xs2: u64, funct7: u32) {
    let selection = state::selection();
    command::execute(
        selection.accelerator_id,
        selection.chip_id,
        xs1,
        xs2,
        funct7,
        WaitMode::Accepted,
        DmaOperation::None,
    )
    .unwrap_or_else(|error| panic!("rushB custom command failed: {error}"));
}

#[no_mangle]
pub extern "C" fn rushb_cycles() -> u64 {
    state::cycles()
}
