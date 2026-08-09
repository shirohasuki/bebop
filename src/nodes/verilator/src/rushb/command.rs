use super::state::HostState;
use crate::ffi::{
    verilator_rushb_accepted, verilator_rushb_complete, verilator_rushb_complete_on_accept, verilator_rushb_last_ready,
    verilator_rushb_last_retired, verilator_rushb_probes, verilator_rushb_submit,
};
use bebop_rushb::FUNCT7_FENCE;

const MAX_STEP_CYCLES: u64 = 100_000_000;

fn step_until(state: &mut HostState, accelerator_id: u32, mut done: impl FnMut() -> bool, context: &str) {
    for _ in 0..MAX_STEP_CYCLES {
        if done() {
            return;
        }
        state.simulator.exec_once();
    }
    unsafe {
        panic!(
            "rushB Verilator timed out while {context}: accelerator={accelerator_id}, probes={}, accepted={}, ready={}, retired={}",
            verilator_rushb_probes(accelerator_id),
            verilator_rushb_accepted(accelerator_id),
            verilator_rushb_last_ready(accelerator_id),
            verilator_rushb_last_retired(accelerator_id),
        );
    }
}

pub(crate) fn execute(state: &mut HostState, accelerator_id: u32, xs1: u64, xs2: u64, funct7: u32) {
    let command_index = state.commands_submitted;
    state.commands_submitted += 1;
    let command = format!("host command #{command_index}: funct7={funct7} xs1=0x{xs1:016x} xs2=0x{xs2:016x}");

    unsafe {
        let accepted_before = verilator_rushb_accepted(accelerator_id);
        verilator_rushb_submit(accelerator_id, xs1, xs2, funct7);
        step_until(
            state,
            accelerator_id,
            || verilator_rushb_accepted(accelerator_id) != accepted_before,
            &format!("waiting for host command acceptance ({command})"),
        );

        // Fences are consumed by Frontend rather than GlobalROB and therefore
        // have no retirement pulse. cmd.fire still serializes subsequent work.
        if funct7 == FUNCT7_FENCE {
            verilator_rushb_complete_on_accept(accelerator_id);
            return;
        }
        step_until(
            state,
            accelerator_id,
            || verilator_rushb_complete(accelerator_id),
            &format!("waiting for accelerator completion ({command})"),
        );
    }
}
