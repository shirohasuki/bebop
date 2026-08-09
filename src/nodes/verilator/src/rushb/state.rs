use crate::ffi::verilator_rushb_clear;
use crate::Simulator;
use std::collections::HashMap;
use std::sync::Mutex;

const POST_RESET_SETTLE_CYCLES: u64 = 4_096;

#[derive(Clone, Copy, Default)]
pub(crate) struct BankConfig {
    pub(crate) allocated: bool,
    pub(crate) groups: u64,
}

pub(crate) struct AcceleratorState {
    pub(crate) chip_id: i32,
    pub(crate) banks: Vec<BankConfig>,
}

impl Default for AcceleratorState {
    fn default() -> Self {
        Self {
            chip_id: 0,
            banks: vec![BankConfig::default(); 1024],
        }
    }
}

pub(crate) struct HostState {
    pub(crate) simulator: Simulator,
    pub(crate) accelerators: HashMap<u32, AcceleratorState>,
    pub(crate) selected_accelerator: u32,
    pub(crate) commands_submitted: u64,
}

// Simulator owns raw C++ pointers but all rushB calls hold HOST_STATE.
unsafe impl Send for HostState {}

static HOST_STATE: Mutex<Option<HostState>> = Mutex::new(None);

pub(crate) fn with_state<R>(f: impl FnOnce(&mut HostState) -> R) -> R {
    let mut slot = HOST_STATE.lock().expect("rushB Verilator state poisoned");
    f(slot.as_mut().expect("rushB is not initialized; call rushb_init first"))
}

pub(crate) fn accelerator_mut(state: &mut HostState, accelerator_id: u32) -> &mut AcceleratorState {
    state.accelerators.entry(accelerator_id).or_default()
}

pub(crate) fn init() {
    let mut slot = HOST_STATE.lock().expect("rushB Verilator state poisoned");
    assert!(slot.is_none(), "rushB Verilator is already initialized");
    unsafe { verilator_rushb_clear() };

    let mut simulator = Simulator::new(None, &[]).expect("failed to create rushB Verilator simulator");
    // Reset must reach BBSimDRAM before it can allocate its DPI backing store.
    for _ in 0..POST_RESET_SETTLE_CYCLES {
        simulator.exec_once();
    }
    *slot = Some(HostState {
        simulator,
        accelerators: HashMap::new(),
        selected_accelerator: 0,
        commands_submitted: 0,
    });
}

pub(crate) fn destroy() {
    let mut slot = HOST_STATE.lock().expect("rushB Verilator state poisoned");
    if let Some(mut state) = slot.take() {
        state.simulator.finalize();
    }
    unsafe { verilator_rushb_clear() };
}

pub(crate) fn cycles() -> u64 {
    with_state(|state| unsafe { crate::ffi::verilator_context_time(state.simulator.context_for_rushb()) / 2 })
}
