use super::command::SchedulerMessage;
use super::scheduler;
use std::cell::Cell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct BankConfig {
    pub(crate) allocated: bool,
    pub(crate) groups: u64,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Selection {
    pub(crate) accelerator_id: u32,
    pub(crate) chip_id: i32,
}

struct SchedulerHandle {
    sender: mpsc::Sender<SchedulerMessage>,
    cycles: Arc<AtomicU64>,
    worker: JoinHandle<Result<(), String>>,
}

type BankConfigs = HashMap<(u32, i32), Vec<BankConfig>>;

static SCHEDULER: Mutex<Option<SchedulerHandle>> = Mutex::new(None);
static COMMAND_ID: AtomicU64 = AtomicU64::new(0);
static BANK_CONFIGS: Mutex<Option<BankConfigs>> = Mutex::new(None);

thread_local! {
    static SELECTION: Cell<Selection> = Cell::new(Selection::default());
}

pub(crate) fn init() {
    let mut slot = SCHEDULER.lock().expect("rushB scheduler state poisoned");
    assert!(slot.is_none(), "rushB Verilator is already initialized");

    COMMAND_ID.store(0, Ordering::Relaxed);
    *BANK_CONFIGS.lock().expect("rushB bank metadata poisoned") = Some(HashMap::new());
    SELECTION.with(|selection| selection.set(Selection::default()));

    let (sender, receiver) = mpsc::channel();
    let (ready_sender, ready_receiver) = mpsc::channel();
    let cycles = Arc::new(AtomicU64::new(0));
    let worker_cycles = Arc::clone(&cycles);
    let worker = std::thread::Builder::new()
        .name("rushb-npu-scheduler".to_string())
        .spawn(move || {
            let result = scheduler::run(receiver, worker_cycles, ready_sender);
            if let Err(error) = &result {
                eprintln!("rushB NPU scheduler failed: {error}");
            }
            result
        })
        .expect("failed to start rushB NPU scheduler thread");

    match ready_receiver.recv() {
        Ok(Ok(())) => {
            *slot = Some(SchedulerHandle { sender, cycles, worker });
        }
        Ok(Err(error)) => {
            let _ = worker.join();
            panic!("failed to initialize rushB NPU scheduler: {error}");
        }
        Err(_) => {
            let _ = worker.join();
            panic!("rushB NPU scheduler stopped during initialization");
        }
    }
}

pub(crate) fn destroy() {
    let handle = SCHEDULER
        .lock()
        .expect("rushB scheduler state poisoned")
        .take()
        .expect("rushB is not initialized; call rushb_init first");
    let (reply, receiver) = mpsc::channel();
    handle
        .sender
        .send(SchedulerMessage::Shutdown(reply))
        .expect("rushB NPU scheduler stopped before shutdown");
    receiver
        .recv()
        .expect("rushB NPU scheduler stopped during shutdown")
        .unwrap_or_else(|error| panic!("rushB NPU scheduler shutdown failed: {error}"));
    handle
        .worker
        .join()
        .expect("rushB NPU scheduler thread panicked")
        .unwrap_or_else(|error| panic!("rushB NPU scheduler failed: {error}"));
    *BANK_CONFIGS.lock().expect("rushB bank metadata poisoned") = None;
}

pub(crate) fn send(message: SchedulerMessage) -> Result<(), String> {
    let sender = SCHEDULER
        .lock()
        .map_err(|_| "rushB scheduler state poisoned".to_string())?
        .as_ref()
        .ok_or_else(|| "rushB is not initialized; call rushb_init first".to_string())?
        .sender
        .clone();
    sender
        .send(message)
        .map_err(|_| "rushB NPU scheduler is not running".to_string())
}

pub(crate) fn cycles() -> u64 {
    SCHEDULER
        .lock()
        .expect("rushB scheduler state poisoned")
        .as_ref()
        .expect("rushB is not initialized; call rushb_init first")
        .cycles
        .load(Ordering::Relaxed)
}

pub(crate) fn next_command_id() -> u64 {
    COMMAND_ID.fetch_add(1, Ordering::Relaxed)
}

pub(crate) fn select(accelerator_id: u32, chip_id: i32) {
    SELECTION.with(|selection| {
        selection.set(Selection {
            accelerator_id,
            chip_id,
        });
    });
}

pub(crate) fn selection() -> Selection {
    SELECTION.with(Cell::get)
}

pub(crate) fn bank_config(selection: Selection, bank_id: usize) -> BankConfig {
    let mut guard = BANK_CONFIGS.lock().expect("rushB bank metadata poisoned");
    let configs = guard
        .as_mut()
        .expect("rushB is not initialized; call rushb_init first")
        .entry((selection.accelerator_id, selection.chip_id))
        .or_insert_with(|| vec![BankConfig::default(); 1024]);
    configs[bank_id]
}

pub(crate) fn update_bank_config(selection: Selection, bank_id: usize, config: BankConfig) {
    let mut guard = BANK_CONFIGS.lock().expect("rushB bank metadata poisoned");
    let configs = guard
        .as_mut()
        .expect("rushB is not initialized; call rushb_init first")
        .entry((selection.accelerator_id, selection.chip_id))
        .or_insert_with(|| vec![BankConfig::default(); 1024]);
    configs[bank_id] = config;
}
