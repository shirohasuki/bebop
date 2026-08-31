use super::dma::{self, PreparedDma, StagingAllocator};
use crate::ffi::{
    verilator_context_time, verilator_rushb_accepted, verilator_rushb_clear, verilator_rushb_complete_on_accept,
    verilator_rushb_completed, verilator_rushb_inflight, verilator_rushb_last_ready, verilator_rushb_last_retired,
    verilator_rushb_probes, verilator_rushb_submit,
};
use crate::Simulator;
use bebop_rushb::{
    CommandId, DmaOperation, RushBackend, RushCommand, RushEvent, RushEventKind, RushMessage, RushResponse,
    RushRuntime, FUNCT7_FENCE,
};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};

const POST_RESET_SETTLE_CYCLES: u64 = 4_096;
const MAX_WAIT_CYCLES: u64 = 100_000_000;

struct PendingCommand {
    id: CommandId,
    accepted_before: u64,
    fence: bool,
}

struct ChannelState {
    pending: Option<PendingCommand>,
    inflight: VecDeque<CommandId>,
    accepted_seen: u64,
    completed_seen: u64,
}

struct VerilatorBackend {
    simulator: Simulator,
    staging: StagingAllocator,
    cycles: Arc<AtomicU64>,
    channels: HashMap<u32, ChannelState>,
    events: VecDeque<RushEvent>,
}

impl VerilatorBackend {
    fn new(cycles: Arc<AtomicU64>) -> Result<Self, String> {
        unsafe { verilator_rushb_clear() };
        let mut simulator = Simulator::new(None, &[])
            .map_err(|error| format!("failed to create rushB Verilator simulator: {error}"))?;
        // Reset must reach BBSimDRAM before it can allocate its DPI backing store.
        for _ in 0..POST_RESET_SETTLE_CYCLES {
            simulator.exec_once();
        }
        let backend = Self {
            simulator,
            staging: StagingAllocator::default(),
            cycles,
            channels: HashMap::new(),
            events: VecDeque::new(),
        };
        backend.update_cycles();
        Ok(backend)
    }

    fn update_cycles(&self) {
        self.cycles.store(self.current_cycle(), Ordering::Relaxed);
    }

    fn current_cycle(&self) -> u64 {
        unsafe { verilator_context_time(self.simulator.context_for_rushb()) / 2 }
    }

    fn poll_hardware(&mut self) -> Result<(), String> {
        let core_ids = self.channels.keys().copied().collect::<Vec<_>>();
        for core_id in core_ids {
            let channel = self.channels.get_mut(&core_id).expect("rushB channel exists");
            let accepted = unsafe { verilator_rushb_accepted(core_id) };
            if accepted < channel.accepted_seen {
                return Err(format!(
                    "rushB accepted counter moved backwards for Core {core_id}: before={} after={accepted}",
                    channel.accepted_seen
                ));
            }
            if accepted > channel.accepted_seen {
                if accepted != channel.accepted_seen + 1 {
                    return Err(format!(
                        "rushB accepted counter skipped for Core {core_id}: before={} after={accepted}",
                        channel.accepted_seen
                    ));
                }
                let pending = channel
                    .pending
                    .take()
                    .ok_or_else(|| format!("rushB Core {core_id} accepted a command that was not submitted"))?;
                if pending.accepted_before != channel.accepted_seen {
                    return Err(format!(
                        "rushB acceptance baseline changed for command {} on Core {core_id}",
                        pending.id
                    ));
                }
                channel.accepted_seen = accepted;
                channel.inflight.push_back(pending.id);
                self.events.push_back(RushEvent {
                    command_id: pending.id,
                    core_id,
                    kind: RushEventKind::Accepted,
                });
                if pending.fence {
                    unsafe { verilator_rushb_complete_on_accept(core_id) };
                }
            }

            let completed = unsafe { verilator_rushb_completed(core_id) };
            if completed < channel.completed_seen {
                return Err(format!(
                    "rushB completed counter moved backwards for Core {core_id}: before={} after={completed}",
                    channel.completed_seen
                ));
            }
            while channel.completed_seen < completed {
                let command_id = channel
                    .inflight
                    .pop_front()
                    .ok_or_else(|| format!("rushB Core {core_id} completed a command that was not in flight"))?;
                channel.completed_seen += 1;
                self.events.push_back(RushEvent {
                    command_id,
                    core_id,
                    kind: RushEventKind::Completed,
                });
            }
        }
        Ok(())
    }
}

impl RushBackend for VerilatorBackend {
    type Prepared = Option<PreparedDma>;

    fn prepare(&mut self, command: &mut RushCommand, dma_operation: DmaOperation) -> Result<Self::Prepared, String> {
        match dma_operation {
            DmaOperation::None => Ok(None),
            DmaOperation::Mvin { spans, chunks } => {
                let address = self.staging.allocate(&spans)?;
                dma::write_staging(address, &chunks)?;
                command.xs2 = dma::staged_xs2(command.xs2, address);
                Ok(Some(PreparedDma {
                    address,
                    spans,
                    output: false,
                }))
            }
            DmaOperation::Mvout { spans } => {
                let address = self.staging.allocate(&spans)?;
                command.xs2 = dma::staged_xs2(command.xs2, address);
                Ok(Some(PreparedDma {
                    address,
                    spans,
                    output: true,
                }))
            }
        }
    }

    fn submit(&mut self, command: &RushCommand) -> Result<(), String> {
        let core_id = command.core_id;
        let accepted = unsafe { verilator_rushb_accepted(core_id) };
        let completed = unsafe { verilator_rushb_completed(core_id) };
        let channel = self.channels.entry(core_id).or_insert_with(|| ChannelState {
            pending: None,
            inflight: VecDeque::new(),
            accepted_seen: accepted,
            completed_seen: completed,
        });
        if channel.pending.is_some() {
            return Err(format!(
                "rushB submitted command {} while Core {core_id} still has a pending command",
                command.id
            ));
        }
        if accepted != channel.accepted_seen || completed != channel.completed_seen {
            return Err(format!(
                "rushB counters changed before command {} was submitted to Core {core_id}",
                command.id
            ));
        }
        channel.pending = Some(PendingCommand {
            id: command.id,
            accepted_before: accepted,
            fence: command.funct7 == FUNCT7_FENCE,
        });
        unsafe { verilator_rushb_submit(core_id, command.xs1, command.xs2, command.funct7) };
        Ok(())
    }

    fn poll_event(&mut self) -> Result<Option<RushEvent>, String> {
        if let Some(event) = self.events.pop_front() {
            return Ok(Some(event));
        }
        self.poll_hardware()?;
        Ok(self.events.pop_front())
    }

    fn finish(&mut self, _command: &RushCommand, prepared: Self::Prepared) -> Result<RushResponse, String> {
        let output = match prepared {
            Some(prepared) if prepared.output => dma::read_staging(prepared.address, &prepared.spans)?,
            _ => Vec::new(),
        };
        Ok(RushResponse { output })
    }

    fn tick(&mut self) -> Result<(), String> {
        self.simulator.exec_once();
        self.update_cycles();
        Ok(())
    }

    fn cycles(&self) -> u64 {
        self.current_cycle()
    }

    fn is_idle(&self) -> bool {
        self.events.is_empty()
            && self.channels.iter().all(|(&core_id, channel)| {
                channel.pending.is_none()
                    && channel.inflight.is_empty()
                    && unsafe { verilator_rushb_inflight(core_id) == 0 }
            })
    }

    fn reset_idle_resources(&mut self) {
        self.staging.reset();
    }

    fn diagnostics(&self, core_id: u32) -> String {
        unsafe {
            format!(
                "probes={} accepted={} completed={} inflight={} ready={} retired={}",
                verilator_rushb_probes(core_id),
                verilator_rushb_accepted(core_id),
                verilator_rushb_completed(core_id),
                verilator_rushb_inflight(core_id),
                verilator_rushb_last_ready(core_id),
                verilator_rushb_last_retired(core_id),
            )
        }
    }

    fn shutdown(&mut self) -> Result<(), String> {
        self.simulator.finalize();
        unsafe { verilator_rushb_clear() };
        Ok(())
    }
}

pub(crate) fn run(
    receiver: mpsc::Receiver<RushMessage>,
    cycles: Arc<AtomicU64>,
    ready: mpsc::Sender<Result<(), String>>,
) -> Result<(), String> {
    let backend = match VerilatorBackend::new(cycles) {
        Ok(backend) => backend,
        Err(error) => {
            let _ = ready.send(Err(error.clone()));
            return Err(error);
        }
    };
    ready
        .send(Ok(()))
        .map_err(|_| "rushB host disappeared during runtime initialization".to_string())?;
    RushRuntime::new(backend, MAX_WAIT_CYCLES).run(receiver)
}
