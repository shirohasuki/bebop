use super::state;
use bebop_rushb::{DmaOperation, RushCommand, RushMessage, RushRequest, RushResponse, WaitMode};
use std::sync::mpsc;

pub(crate) fn execute(
    core_id: u32,
    xs1: u64,
    xs2: u64,
    funct7: u32,
    wait: WaitMode,
    dma: DmaOperation,
) -> Result<RushResponse, String> {
    let command_id = state::next_command_id();
    let (response, receiver) = mpsc::channel();
    let request = RushRequest {
        command: RushCommand {
            id: command_id,
            core_id,
            xs1,
            xs2,
            funct7,
        },
        wait,
        dma,
        response,
    };
    state::send(RushMessage::Command(request))?;
    receiver
        .recv()
        .map_err(|_| format!("rushB NPU scheduler stopped while waiting for host command #{command_id}"))?
}
