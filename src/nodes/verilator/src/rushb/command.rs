use super::dma::{DmaChunk, DmaOperation};
use super::state;
use std::sync::mpsc;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WaitMode {
    Accepted,
    Completed,
}

pub(crate) struct CommandResponse {
    pub(crate) output: Vec<DmaChunk>,
}

pub(crate) struct CommandRequest {
    pub(crate) command_id: u64,
    pub(crate) accelerator_id: u32,
    pub(crate) chip_id: i32,
    pub(crate) xs1: u64,
    pub(crate) xs2: u64,
    pub(crate) funct7: u32,
    pub(crate) wait: WaitMode,
    pub(crate) dma: DmaOperation,
    pub(crate) response: mpsc::Sender<Result<CommandResponse, String>>,
}

pub(crate) enum SchedulerMessage {
    Command(CommandRequest),
    Shutdown(mpsc::Sender<Result<(), String>>),
}

pub(crate) fn execute(
    accelerator_id: u32,
    chip_id: i32,
    xs1: u64,
    xs2: u64,
    funct7: u32,
    wait: WaitMode,
    dma: DmaOperation,
) -> Result<CommandResponse, String> {
    let command_id = state::next_command_id();
    let (response, receiver) = mpsc::channel();
    let request = CommandRequest {
        command_id,
        accelerator_id,
        chip_id,
        xs1,
        xs2,
        funct7,
        wait,
        dma,
        response,
    };
    state::send(SchedulerMessage::Command(request))?;
    receiver
        .recv()
        .map_err(|_| format!("rushB NPU scheduler stopped while waiting for host command #{command_id}"))?
}
