use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::mpsc;

use crate::{CommandId, DmaOperation, RushCommand, RushEvent, RushEventKind, RushResponse, WaitMode};

pub struct RushRequest {
    pub command: RushCommand,
    pub wait: WaitMode,
    pub dma: DmaOperation,
    pub response: mpsc::Sender<Result<RushResponse, String>>,
}

pub enum RushMessage {
    Command(RushRequest),
    Shutdown(mpsc::Sender<Result<(), String>>),
}

/// Backend-specific mechanics used by the backend-neutral scheduler.
pub trait RushBackend {
    type Prepared;

    fn prepare(&mut self, command: &mut RushCommand, dma: DmaOperation) -> Result<Self::Prepared, String>;
    fn submit(&mut self, command: &RushCommand) -> Result<(), String>;
    fn poll_event(&mut self) -> Result<Option<RushEvent>, String>;
    fn finish(&mut self, command: &RushCommand, prepared: Self::Prepared) -> Result<RushResponse, String>;
    fn tick(&mut self) -> Result<(), String>;
    fn cycles(&self) -> u64;
    fn is_idle(&self) -> bool;
    fn reset_idle_resources(&mut self);
    fn diagnostics(&self, core_id: u32) -> String;
    fn shutdown(&mut self) -> Result<(), String>;
}

struct ActiveCommand<P> {
    request: RushRequest,
    prepared: P,
    started_cycle: u64,
}

struct CompletionWait<P> {
    request: RushRequest,
    prepared: P,
    started_cycle: u64,
}

struct CoreQueue<P> {
    queued: VecDeque<RushRequest>,
    active: Option<ActiveCommand<P>>,
    completion_wait: Option<CompletionWait<P>>,
}

impl<P> Default for CoreQueue<P> {
    fn default() -> Self {
        Self {
            queued: VecDeque::new(),
            active: None,
            completion_wait: None,
        }
    }
}

pub struct RushRuntime<B: RushBackend> {
    backend: B,
    queues: HashMap<u32, CoreQueue<B::Prepared>>,
    accepted_only: HashMap<CommandId, u32>,
    outstanding: HashSet<CommandId>,
    max_wait_cycles: u64,
    resources_idle: bool,
}

impl<B: RushBackend> RushRuntime<B> {
    pub fn new(backend: B, max_wait_cycles: u64) -> Self {
        Self {
            backend,
            queues: HashMap::new(),
            accepted_only: HashMap::new(),
            outstanding: HashSet::new(),
            max_wait_cycles,
            resources_idle: false,
        }
    }

    pub fn run(mut self, receiver: mpsc::Receiver<RushMessage>) -> Result<(), String> {
        let mut shutdown = None;
        loop {
            self.drain_messages(&receiver, &mut shutdown)?;
            self.process_events()?;
            self.submit_available()?;

            let idle = self.is_drained();
            if idle && !self.resources_idle {
                self.backend.reset_idle_resources();
            }
            self.resources_idle = idle;

            if let Some(reply) = shutdown.take() {
                if idle {
                    let result = self.backend.shutdown();
                    let _ = reply.send(result.clone());
                    return result;
                }
                shutdown = Some(reply);
            }

            if !self.has_runtime_work() {
                let message = receiver
                    .recv()
                    .map_err(|_| "rushB host disconnected without shutting down the runtime".to_string())?;
                self.handle_message(message, &mut shutdown)?;
                continue;
            }

            self.backend.tick()?;
            self.process_events()?;
            self.check_timeouts()?;
        }
    }

    fn drain_messages(
        &mut self,
        receiver: &mpsc::Receiver<RushMessage>,
        shutdown: &mut Option<mpsc::Sender<Result<(), String>>>,
    ) -> Result<(), String> {
        while let Ok(message) = receiver.try_recv() {
            self.handle_message(message, shutdown)?;
        }
        Ok(())
    }

    fn handle_message(
        &mut self,
        message: RushMessage,
        shutdown: &mut Option<mpsc::Sender<Result<(), String>>>,
    ) -> Result<(), String> {
        match message {
            RushMessage::Command(request) => {
                if shutdown.is_some() {
                    let _ = request.response.send(Err("rushB runtime is shutting down".to_string()));
                } else if !self.outstanding.insert(request.command.id) {
                    let _ = request
                        .response
                        .send(Err(format!("duplicate rushB command id {}", request.command.id)));
                } else {
                    self.queues
                        .entry(request.command.core_id)
                        .or_default()
                        .queued
                        .push_back(request);
                    self.resources_idle = false;
                }
            }
            RushMessage::Shutdown(reply) => {
                if shutdown.replace(reply).is_some() {
                    return Err("duplicate rushB runtime shutdown request".to_string());
                }
            }
        }
        Ok(())
    }

    fn submit_available(&mut self) -> Result<(), String> {
        let core_ids = self.queues.keys().copied().collect::<Vec<_>>();
        for core_id in core_ids {
            let queue = self.queues.get_mut(&core_id).expect("Core queue exists");
            if queue.active.is_some() || queue.completion_wait.is_some() {
                continue;
            }
            let Some(mut request) = queue.queued.pop_front() else {
                continue;
            };
            let dma = std::mem::replace(&mut request.dma, DmaOperation::None);
            let prepared = self.backend.prepare(&mut request.command, dma)?;
            self.backend.submit(&request.command)?;
            queue.active = Some(ActiveCommand {
                request,
                prepared,
                started_cycle: self.backend.cycles(),
            });
        }
        Ok(())
    }

    fn process_events(&mut self) -> Result<(), String> {
        while let Some(event) = self.backend.poll_event()? {
            match event.kind {
                RushEventKind::Accepted => self.process_accepted(event)?,
                RushEventKind::Completed => self.process_completed(event)?,
            }
        }
        Ok(())
    }

    fn process_accepted(&mut self, event: RushEvent) -> Result<(), String> {
        let queue = self
            .queues
            .get_mut(&event.core_id)
            .ok_or_else(|| format!("rushB accepted command {} for an unknown Core", event.command_id))?;
        let active = queue
            .active
            .take()
            .ok_or_else(|| format!("rushB accepted command {} with no pending command", event.command_id))?;
        if active.request.command.id != event.command_id {
            return Err(format!(
                "rushB acceptance id mismatch: expected={} actual={}",
                active.request.command.id, event.command_id
            ));
        }

        match active.request.wait {
            WaitMode::Accepted => {
                let response = self.backend.finish(&active.request.command, active.prepared)?;
                self.accepted_only.insert(active.request.command.id, event.core_id);
                let _ = active.request.response.send(Ok(response));
            }
            WaitMode::Completed => {
                queue.completion_wait = Some(CompletionWait {
                    request: active.request,
                    prepared: active.prepared,
                    started_cycle: self.backend.cycles(),
                });
            }
        }
        Ok(())
    }

    fn process_completed(&mut self, event: RushEvent) -> Result<(), String> {
        let queue = self
            .queues
            .get_mut(&event.core_id)
            .ok_or_else(|| format!("rushB completed command {} for an unknown Core", event.command_id))?;
        let is_waiting = queue
            .completion_wait
            .as_ref()
            .is_some_and(|waiting| waiting.request.command.id == event.command_id);
        if is_waiting {
            let waiting = queue.completion_wait.take().expect("matching completion waiter exists");
            let response = self.backend.finish(&waiting.request.command, waiting.prepared)?;
            self.outstanding.remove(&event.command_id);
            let _ = waiting.request.response.send(Ok(response));
            return Ok(());
        }

        // Accepted-only calls still produce completion events. Consume those
        // events so the backend can track all in-flight commands precisely.
        let core_id = self
            .accepted_only
            .remove(&event.command_id)
            .ok_or_else(|| format!("rushB completed unknown or duplicate command {}", event.command_id))?;
        if core_id != event.core_id {
            return Err(format!(
                "rushB completion Core mismatch for command {}",
                event.command_id
            ));
        }
        self.outstanding.remove(&event.command_id);
        Ok(())
    }

    fn check_timeouts(&self) -> Result<(), String> {
        let cycle = self.backend.cycles();
        for (&core_id, queue) in &self.queues {
            if let Some(active) = &queue.active {
                if cycle.saturating_sub(active.started_cycle) >= self.max_wait_cycles {
                    return Err(self.timeout_message(core_id, &active.request.command, "acceptance"));
                }
            }
            if let Some(waiting) = &queue.completion_wait {
                if cycle.saturating_sub(waiting.started_cycle) >= self.max_wait_cycles {
                    return Err(self.timeout_message(core_id, &waiting.request.command, "completion"));
                }
            }
        }
        Ok(())
    }

    fn timeout_message(&self, core_id: u32, command: &RushCommand, phase: &str) -> String {
        format!(
            "rushB runtime timed out waiting for {phase}: command={} core={} funct7={} xs1=0x{:016x} xs2=0x{:016x} {}",
            command.id,
            core_id,
            command.funct7,
            command.xs1,
            command.xs2,
            self.backend.diagnostics(core_id),
        )
    }

    fn is_drained(&self) -> bool {
        self.queues
            .values()
            .all(|queue| queue.queued.is_empty() && queue.active.is_none() && queue.completion_wait.is_none())
            && self.accepted_only.is_empty()
            && self.backend.is_idle()
    }

    fn has_runtime_work(&self) -> bool {
        self.queues
            .values()
            .any(|queue| !queue.queued.is_empty() || queue.active.is_some() || queue.completion_wait.is_some())
            || !self.accepted_only.is_empty()
            || !self.backend.is_idle()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};
    use std::thread;

    use super::*;

    #[derive(Default)]
    struct FakeState {
        submitted: Vec<(CommandId, u32)>,
        cycles: u64,
        shutdown: bool,
    }

    struct FakeBackend {
        state: Arc<Mutex<FakeState>>,
        events: VecDeque<RushEvent>,
        inflight: usize,
    }

    impl RushBackend for FakeBackend {
        type Prepared = ();

        fn prepare(&mut self, _command: &mut RushCommand, _dma: DmaOperation) -> Result<Self::Prepared, String> {
            Ok(())
        }

        fn submit(&mut self, command: &RushCommand) -> Result<(), String> {
            self.state.lock().unwrap().submitted.push((command.id, command.core_id));
            self.inflight += 1;
            self.events.push_back(RushEvent {
                command_id: command.id,
                core_id: command.core_id,
                kind: RushEventKind::Accepted,
            });
            self.events.push_back(RushEvent {
                command_id: command.id,
                core_id: command.core_id,
                kind: RushEventKind::Completed,
            });
            Ok(())
        }

        fn poll_event(&mut self) -> Result<Option<RushEvent>, String> {
            let event = self.events.pop_front();
            if event.is_some_and(|event| event.kind == RushEventKind::Completed) {
                self.inflight -= 1;
            }
            Ok(event)
        }

        fn finish(&mut self, _command: &RushCommand, _prepared: Self::Prepared) -> Result<RushResponse, String> {
            Ok(RushResponse::default())
        }

        fn tick(&mut self) -> Result<(), String> {
            self.state.lock().unwrap().cycles += 1;
            Ok(())
        }

        fn cycles(&self) -> u64 {
            self.state.lock().unwrap().cycles
        }

        fn is_idle(&self) -> bool {
            self.inflight == 0 && self.events.is_empty()
        }

        fn reset_idle_resources(&mut self) {}

        fn diagnostics(&self, _core_id: u32) -> String {
            "fake-backend".to_string()
        }

        fn shutdown(&mut self) -> Result<(), String> {
            self.state.lock().unwrap().shutdown = true;
            Ok(())
        }
    }

    #[test]
    fn schedules_commands_by_core() {
        let state = Arc::new(Mutex::new(FakeState::default()));
        let backend = FakeBackend {
            state: Arc::clone(&state),
            events: VecDeque::new(),
            inflight: 0,
        };
        let (sender, receiver) = mpsc::channel();
        let worker = thread::spawn(move || RushRuntime::new(backend, 100).run(receiver));

        let (response, result) = mpsc::channel();
        sender
            .send(RushMessage::Command(RushRequest {
                command: RushCommand {
                    id: 7,
                    core_id: 65_536,
                    xs1: 1,
                    xs2: 2,
                    funct7: 3,
                },
                wait: WaitMode::Completed,
                dma: DmaOperation::None,
                response,
            }))
            .unwrap();
        result.recv().unwrap().unwrap();

        let (shutdown, done) = mpsc::channel();
        sender.send(RushMessage::Shutdown(shutdown)).unwrap();
        done.recv().unwrap().unwrap();
        worker.join().unwrap().unwrap();

        let state = state.lock().unwrap();
        assert_eq!(state.submitted, vec![(7, 65_536)]);
        assert!(state.shutdown);
    }

    #[test]
    fn rejects_unknown_completion_ids() {
        let state = Arc::new(Mutex::new(FakeState::default()));
        let mut events = VecDeque::new();
        events.push_back(RushEvent {
            command_id: 99,
            core_id: 0,
            kind: RushEventKind::Completed,
        });
        let backend = FakeBackend {
            state,
            events,
            inflight: 1,
        };
        let (_sender, receiver) = mpsc::channel();
        let error = RushRuntime::new(backend, 100).run(receiver).unwrap_err();
        assert!(error.contains("unknown Core") || error.contains("unknown or duplicate command 99"));
    }
}
