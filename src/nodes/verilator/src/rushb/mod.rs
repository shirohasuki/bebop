mod command;
mod dma;
mod state;

use bebop_rushb::{FUNCT7_MSET, FUNCT7_MVIN, FUNCT7_MVOUT};
use std::ffi::c_void;

#[no_mangle]
pub extern "C" fn rushb_init() {
    state::init();
}

#[no_mangle]
pub extern "C" fn rushb_destroy() {
    state::destroy();
}

#[no_mangle]
pub extern "C" fn rushb_select_accelerator(accelerator_id: u32, chip_id: i32) {
    state::with_state(|state| {
        state::accelerator_mut(state, accelerator_id).chip_id = chip_id;
        state.selected_accelerator = accelerator_id;
    });
}

#[no_mangle]
pub extern "C" fn rushb_mset(xs1: u64, xs2: u64) {
    state::with_state(|state| {
        let accelerator_id = state.selected_accelerator;
        command::execute(state, accelerator_id, xs1, xs2, FUNCT7_MSET);

        let bank_id = usize::try_from(xs1 & 0x3ff).expect("invalid bank id");
        let raw_cols = (xs2 >> 5) & 0x1f;
        let allocated = ((xs2 >> 10) & 1) != 0;
        let bank = &mut state::accelerator_mut(state, accelerator_id).banks[bank_id];
        bank.allocated = allocated;
        // col=0 represents the elaborated accelerator's full bank width. The
        // generic host runtime cannot infer that width, so reject its DMA use.
        bank.groups = if allocated && raw_cols != 0 { raw_cols } else { 0 };
    });
}

#[no_mangle]
pub extern "C" fn rushb_mvin(xs1: u64, packed_xs2: u64, host_ptr: *const c_void) {
    state::with_state(|state| {
        let accelerator_id = state.selected_accelerator;
        let bank_id = usize::try_from(xs1 & 0x3ff).expect("invalid bank id");
        let config = state::accelerator_mut(state, accelerator_id).banks[bank_id];
        let spans = dma::spans(config, xs1, packed_xs2);
        let chip_id = state::accelerator_mut(state, accelerator_id).chip_id;
        let address = dma::staging_address(chip_id, &spans);
        unsafe { dma::copy_to_staging(chip_id, address, host_ptr.cast(), &spans) };
        command::execute(
            state,
            accelerator_id,
            xs1,
            dma::staged_xs2(packed_xs2, address),
            FUNCT7_MVIN,
        );
    });
}

#[no_mangle]
pub extern "C" fn rushb_mvout(xs1: u64, packed_xs2: u64, host_ptr: *mut c_void) {
    state::with_state(|state| {
        let accelerator_id = state.selected_accelerator;
        let bank_id = usize::try_from(xs1 & 0x3ff).expect("invalid bank id");
        let config = state::accelerator_mut(state, accelerator_id).banks[bank_id];
        let spans = dma::spans(config, xs1, packed_xs2);
        let chip_id = state::accelerator_mut(state, accelerator_id).chip_id;
        let address = dma::staging_address(chip_id, &spans);
        command::execute(
            state,
            accelerator_id,
            xs1,
            dma::staged_xs2(packed_xs2, address),
            FUNCT7_MVOUT,
        );
        unsafe { dma::copy_from_staging(chip_id, address, host_ptr.cast(), &spans) };
    });
}

#[no_mangle]
pub extern "C" fn rushb_custom(xs1: u64, xs2: u64, funct7: u32) {
    state::with_state(|state| {
        let accelerator_id = state.selected_accelerator;
        command::execute(state, accelerator_id, xs1, xs2, funct7);
    });
}

#[no_mangle]
pub extern "C" fn rushb_cycles() -> u64 {
    state::cycles()
}
