use super::command::{CommandRequest, CommandResponse, SchedulerMessage, WaitMode};
use super::dma::{self, DmaOperation, PreparedDma, StagingAllocator};
use crate::ffi::{
    verilator_context_time, verilator_rushb_accepted, verilator_rushb_clear, verilator_rushb_complete_on_accept,
    verilator_rushb_completed, verilator_rushb_inflight, verilator_rushb_last_ready, verilator_rushb_last_retired,
    verilator_rushb_probes, verilator_rushb_submit,
};
use crate::Simulator;
use bebop_rushb::FUNCT7_FENCE;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};

const POST_RESET_SETTLE_CYCLES: u64 = 4_096;
const MAX_WAIT_CYCLES: u64 = 100_000_000;

#[derive(Default)]
struct AcceleratorQueue {
    queued: VecDeque<CommandRequest>,
    active: Option<ActiveCommand>,
    completion_wait: Option<CompletionWait>,
}

struct ActiveCommand {
    request: CommandRequest,
    accepted_before: u64,
    prepared_dma: Option<PreparedDma>,
    started_cycle: u64,
}

struct CompletionWait {
    request: CommandRequest,
    target_completed: u64,
    prepared_dma: Option<PreparedDma>,
    started_cycle: u64,
}

pub(crate) fn run(
    receiver: mpsc::Receiver<SchedulerMessage>,
    cycles: Arc<AtomicU64>,
    ready: mpsc::Sender<Result<(), String>>,
) -> Result<(), String> {
    unsafe { verilator_rushb_clear() };
    let mut simulator = match Simulator::new(None, &[]) {
        Ok(simulator) => simulator,
        Err(error) => {
            let message = format!("failed to create rushB Verilator simulator: {error}");
            let _ = ready.send(Err(message.clone()));
            return Err(message);
        }
    };
    for _ in 0..POST_RESET_SETTLE_CYCLES {
        simulator.exec_once();
    }
    update_cycles(&simulator, &cycles);
    ready
        .send(Ok(()))
        .map_err(|_| "rushB host disappeared during scheduler initialization".to_string())?;

    let mut queues = HashMap::<u32, AcceleratorQueue>::new();
    let mut staging = StagingAllocator::default();
    let mut shutdown = None;

    loop {
        drain_messages(&receiver, &mut queues, &mut shutdown)?;
        process_completions(&mut queues, current_cycle(&simulator))?;

        if all_inflight_zero(&queues) && queues.values().all(|queue| queue.active.is_none()) {
            staging.reset();
        }
        submit_available(&mut queues, &mut staging, current_cycle(&simulator))?;

        if let Some(reply) = shutdown.take() {
            if is_drained(&queues) {
                simulator.finalize();
                unsafe { verilator_rushb_clear() };
                let _ = reply.send(Ok(()));
                return Ok(());
            }
            shutdown = Some(reply);
        }

        if !has_runtime_work(&queues) {
            let message = receiver
                .recv()
                .map_err(|_| "rushB host disconnected without shutting down the NPU scheduler".to_string())?;
            handle_message(message, &mut queues, &mut shutdown)?;
            continue;
        }

        simulator.exec_once();
        update_cycles(&simulator, &cycles);
        let cycle = current_cycle(&simulator);
        process_accepts(&mut queues, cycle)?;
        process_completions(&mut queues, cycle)?;
        check_timeouts(&queues, cycle)?;
    }
}

fn drain_messages(
    receiver: &mpsc::Receiver<SchedulerMessage>,
    queues: &mut HashMap<u32, AcceleratorQueue>,
    shutdown: &mut Option<mpsc::Sender<Result<(), String>>>,
) -> Result<(), String> {
    while let Ok(message) = receiver.try_recv() {
        handle_message(message, queues, shutdown)?;
    }
    Ok(())
}

fn handle_message(
    message: SchedulerMessage,
    queues: &mut HashMap<u32, AcceleratorQueue>,
    shutdown: &mut Option<mpsc::Sender<Result<(), String>>>,
) -> Result<(), String> {
    match message {
        SchedulerMessage::Command(request) => {
            if shutdown.is_some() {
                let _ = request
                    .response
                    .send(Err("rushB NPU scheduler is shutting down".to_string()));
            } else {
                queues
                    .entry(request.accelerator_id)
                    .or_default()
                    .queued
                    .push_back(request);
            }
        }
        SchedulerMessage::Shutdown(reply) => {
            if shutdown.replace(reply).is_some() {
                return Err("duplicate rushB NPU scheduler shutdown request".to_string());
            }
        }
    }
    Ok(())
}

fn submit_available(
    queues: &mut HashMap<u32, AcceleratorQueue>,
    staging: &mut StagingAllocator,
    cycle: u64,
) -> Result<(), String> {
    let accelerator_ids = queues.keys().copied().collect::<Vec<_>>();
    for accelerator_id in accelerator_ids {
        let queue = queues.get_mut(&accelerator_id).expect("accelerator queue exists");
        if queue.active.is_some() || queue.completion_wait.is_some() {
            continue;
        }
        let Some(mut request) = queue.queued.pop_front() else {
            continue;
        };

        let dma_operation = std::mem::replace(&mut request.dma, DmaOperation::None);
        let prepared_dma_result = match dma_operation {
            DmaOperation::None => Ok(None),
            DmaOperation::Mvin { spans, chunks } => staging.allocate(request.chip_id, &spans).and_then(|address| {
                dma::write_staging(request.chip_id, address, &chunks)?;
                request.xs2 = dma::staged_xs2(request.xs2, address);
                Ok(Some(PreparedDma {
                    address,
                    spans,
                    output: false,
                }))
            }),
            DmaOperation::Mvout { spans } => staging.allocate(request.chip_id, &spans).map(|address| {
                request.xs2 = dma::staged_xs2(request.xs2, address);
                Some(PreparedDma {
                    address,
                    spans,
                    output: true,
                })
            }),
        };
        let prepared_dma = match prepared_dma_result {
            Ok(prepared) => prepared,
            Err(error) => {
                let message = format!(
                    "failed to prepare rushB DMA for command {}: {error}",
                    request.command_id
                );
                let _ = request.response.send(Err(message.clone()));
                return Err(message);
            }
        };

        let accepted_before = unsafe { verilator_rushb_accepted(accelerator_id) };
        unsafe { verilator_rushb_submit(accelerator_id, request.xs1, request.xs2, request.funct7) };
        queue.active = Some(ActiveCommand {
            request,
            accepted_before,
            prepared_dma,
            started_cycle: cycle,
        });
    }
    Ok(())
}

fn process_accepts(queues: &mut HashMap<u32, AcceleratorQueue>, cycle: u64) -> Result<(), String> {
    let accelerator_ids = queues.keys().copied().collect::<Vec<_>>();
    for accelerator_id in accelerator_ids {
        let queue = queues.get_mut(&accelerator_id).expect("accelerator queue exists");
        let Some(active) = queue.active.as_ref() else {
            continue;
        };
        let accepted = unsafe { verilator_rushb_accepted(accelerator_id) };
        if accepted == active.accepted_before {
            continue;
        }
        if accepted != active.accepted_before + 1 {
            return Err(format!(
                "rushB accepted counter skipped for accelerator {accelerator_id}: before={} after={accepted}",
                active.accepted_before
            ));
        }

        let active = queue.active.take().expect("active command exists");
        if active.request.funct7 == FUNCT7_FENCE {
            unsafe { verilator_rushb_complete_on_accept(accelerator_id) };
        }
        match active.request.wait {
            WaitMode::Accepted => {
                let _ = active.request.response.send(Ok(CommandResponse { output: Vec::new() }));
            }
            WaitMode::Completed => {
                queue.completion_wait = Some(CompletionWait {
                    request: active.request,
                    target_completed: accepted,
                    prepared_dma: active.prepared_dma,
                    started_cycle: cycle,
                });
            }
        }
    }
    Ok(())
}

fn process_completions(queues: &mut HashMap<u32, AcceleratorQueue>, _cycle: u64) -> Result<(), String> {
    let accelerator_ids = queues.keys().copied().collect::<Vec<_>>();
    for accelerator_id in accelerator_ids {
        let queue = queues.get_mut(&accelerator_id).expect("accelerator queue exists");
        let Some(waiting) = queue.completion_wait.as_ref() else {
            continue;
        };
        let completed = unsafe { verilator_rushb_completed(accelerator_id) };
        if completed < waiting.target_completed {
            continue;
        }
        let waiting = queue.completion_wait.take().expect("completion waiter exists");
        let output = match waiting.prepared_dma {
            Some(prepared) if prepared.output => {
                dma::read_staging(waiting.request.chip_id, prepared.address, &prepared.spans)?
            }
            _ => Vec::new(),
        };
        let _ = waiting.request.response.send(Ok(CommandResponse { output }));
    }
    Ok(())
}

fn check_timeouts(queues: &HashMap<u32, AcceleratorQueue>, cycle: u64) -> Result<(), String> {
    for (&accelerator_id, queue) in queues {
        if let Some(active) = &queue.active {
            if cycle.saturating_sub(active.started_cycle) >= MAX_WAIT_CYCLES {
                return Err(timeout_message(accelerator_id, &active.request, "acceptance"));
            }
        }
        if let Some(waiting) = &queue.completion_wait {
            if cycle.saturating_sub(waiting.started_cycle) >= MAX_WAIT_CYCLES {
                return Err(timeout_message(accelerator_id, &waiting.request, "completion"));
            }
        }
    }
    Ok(())
}

fn timeout_message(accelerator_id: u32, request: &CommandRequest, phase: &str) -> String {
    unsafe {
        format!(
            "rushB NPU scheduler timed out waiting for {phase}: command={} accelerator={} funct7={} xs1=0x{:016x} xs2=0x{:016x} probes={} accepted={} completed={} inflight={} ready={} retired={}",
            request.command_id,
            accelerator_id,
            request.funct7,
            request.xs1,
            request.xs2,
            verilator_rushb_probes(accelerator_id),
            verilator_rushb_accepted(accelerator_id),
            verilator_rushb_completed(accelerator_id),
            verilator_rushb_inflight(accelerator_id),
            verilator_rushb_last_ready(accelerator_id),
            verilator_rushb_last_retired(accelerator_id),
        )
    }
}

fn all_inflight_zero(queues: &HashMap<u32, AcceleratorQueue>) -> bool {
    queues
        .keys()
        .all(|&accelerator_id| unsafe { verilator_rushb_inflight(accelerator_id) == 0 })
}

fn is_drained(queues: &HashMap<u32, AcceleratorQueue>) -> bool {
    queues
        .values()
        .all(|queue| queue.queued.is_empty() && queue.active.is_none() && queue.completion_wait.is_none())
        && all_inflight_zero(queues)
}

fn has_runtime_work(queues: &HashMap<u32, AcceleratorQueue>) -> bool {
    queues
        .values()
        .any(|queue| !queue.queued.is_empty() || queue.active.is_some() || queue.completion_wait.is_some())
        || !all_inflight_zero(queues)
}

fn current_cycle(simulator: &Simulator) -> u64 {
    unsafe { verilator_context_time(simulator.context_for_rushb()) / 2 }
}

fn update_cycles(simulator: &Simulator, cycles: &AtomicU64) {
    cycles.store(current_cycle(simulator), Ordering::Relaxed);
}
