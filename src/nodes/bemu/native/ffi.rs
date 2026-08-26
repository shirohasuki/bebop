use bebop_bank_hash::bank_hash;
use bebop_bemu_profile::{BemuProfile, BemuProfileReport};
use bebop_dtb::DtbBuilder;
use bebop_elf::{load_elf, LoadInfo, TlsInfo};
use bebop_rushb::{FUNCT7_MSET, FUNCT7_MVIN, FUNCT7_MVOUT};
use bebop_syscall::{add_guest_mapping, handle_syscall_with_state, set_guest_mappings, SyscallState};
use bebop_uart::Uart;
use std::cell::Cell;
use std::collections::HashMap;
use std::os::raw::{c_char, c_void};
use std::path::Path;
use std::slice;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::bank::{bank_num, bank_size, mmio_bank_num, mmio_bank_size, BankConfig, BankMap, MATRIX_SIZE};
use crate::inst;
use crate::trace::{with_trace_ptr, TraceConfig, TraceState};

const DRAM_BASE: u64 = 0x80000000;
// UART base address (matches test workloads)
const UART_BASE: u64 = 0x60020000;
const PAGE_SIZE: u64 = 4096;
const USER_TOP: u64 = 0x40_0000_0000;
// musl __mmap64 treats syscall results with a0 > 0xfffff000 as errors.
const PK_MMAP_CEILING: u64 = 0x8000_0000;
const USER_STACK_SIZE: u64 = 8 * 1024 * 1024;
const PK_PT_RESERVE: u64 = 16 * 1024 * 1024;
const PK_HIGH_RESERVE: u64 = 64 * 1024 * 1024;
const SYS_BRK: u64 = 214;
const SYS_MUNMAP: u64 = 215;
const SYS_MMAP: u64 = 222;

pub struct SharedMemory {
    data: std::cell::UnsafeCell<Vec<u8>>,
    barrier: TileBarrier,
}

unsafe impl Send for SharedMemory {}
unsafe impl Sync for SharedMemory {}

impl SharedMemory {
    pub fn new(size: usize, core_count: usize) -> Arc<Self> {
        Arc::new(Self {
            data: std::cell::UnsafeCell::new(vec![0; size]),
            barrier: TileBarrier::new(core_count),
        })
    }

    fn as_slice(&self) -> &[u8] {
        unsafe { &*self.data.get() }
    }

    fn as_mut_slice(&self) -> &mut [u8] {
        unsafe { &mut *self.data.get() }
    }

    pub fn wait_barrier(&self, hart_id: usize) {
        self.barrier.wait(hart_id);
    }

    pub fn abort_barrier(&self) {
        self.barrier.abort();
    }
}

struct TileBarrier {
    core_count: usize,
    state: Mutex<(u64, Vec<usize>, bool)>,
    ready: std::sync::Condvar,
}

impl TileBarrier {
    fn new(core_count: usize) -> Self {
        Self {
            core_count,
            state: Mutex::new((0, Vec::with_capacity(core_count), false)),
            ready: std::sync::Condvar::new(),
        }
    }

    fn wait(&self, hart_id: usize) {
        let mut state = self.state.lock().expect("BEMU barrier poisoned");
        let epoch = state.0;
        if state.2 {
            return;
        }
        if !state.1.contains(&hart_id) {
            state.1.push(hart_id);
        }
        if state.1.len() == self.core_count {
            state.0 = state.0.wrapping_add(1);
            state.1.clear();
            self.ready.notify_all();
            return;
        }
        while state.0 == epoch && !state.2 {
            state = self.ready.wait(state).expect("BEMU barrier poisoned");
        }
    }

    fn abort(&self) {
        let mut state = self.state.lock().expect("BEMU barrier poisoned");
        state.2 = true;
        self.ready.notify_all();
    }
}

enum GuestMemory {
    Owned(Vec<u8>),
    Shared(Arc<SharedMemory>),
}

impl GuestMemory {
    fn as_mut_ptr(&mut self) -> *mut u8 {
        match self {
            Self::Owned(memory) => memory.as_mut_ptr(),
            Self::Shared(memory) => memory.as_mut_slice().as_mut_ptr(),
        }
    }

    fn len(&self) -> usize {
        match self {
            Self::Owned(memory) => memory.len(),
            Self::Shared(memory) => memory.as_slice().len(),
        }
    }
}

impl std::ops::Deref for GuestMemory {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Owned(memory) => memory,
            Self::Shared(memory) => memory.as_slice(),
        }
    }
}

impl std::ops::DerefMut for GuestMemory {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::Owned(memory) => memory,
            Self::Shared(memory) => memory.as_mut_slice(),
        }
    }
}

struct EmuState {
    memory: GuestMemory,
    banks: Vec<Vec<u8>>,
    bank_cfgs: Vec<BankConfig>,
    bank_map: BankMap,
    bank_scoreboard: inst::instruction::BankScoreboard,
    deferred_bank_frees: Vec<u32>,
    mmio_banks: Vec<Vec<u8>>,
    total_lat: u64,
    npu_instruction_id: u64,
    matrix_instruction_count: u64,
    uart: Uart,
    syscall: SyscallState,
    pk_vm: Option<PkVm>,
    trace: TraceState,
    profile: BemuProfile,
    barrier_hit: bool,
}

impl EmuState {
    fn new(
        log_dir: &Path,
        trace_config: TraceConfig,
        profile: bool,
        shared_memory: Option<Arc<SharedMemory>>,
    ) -> Result<Self, String> {
        // 1GB Here is important, for baremetal mode, when we set this to 4GB,
        // it will running for a long time.
        const MEM_SIZE: usize = 3 * (1 << 30);
        Ok(Self {
            // memory is maintained by bemu not spike
            memory: shared_memory.map_or_else(|| GuestMemory::Owned(vec![0; MEM_SIZE]), GuestMemory::Shared),
            banks: vec![vec![0; bank_size()]; bank_num()],
            bank_cfgs: vec![BankConfig::default(); bank_num()],
            bank_map: BankMap::new(bank_num()),
            bank_scoreboard: inst::instruction::BankScoreboard::new(),
            deferred_bank_frees: Vec::new(),
            mmio_banks: vec![vec![0; mmio_bank_size()]; mmio_bank_num()],
            total_lat: 0,
            npu_instruction_id: 0,
            matrix_instruction_count: 0,
            uart: Uart::new(),
            syscall: SyscallState::new(),
            pk_vm: None,
            trace: TraceState::new(log_dir, trace_config).map_err(|e| e.to_string())?,
            profile: BemuProfile::new(profile),
            barrier_hit: false,
        })
    }

    fn reset_accel(&mut self) {
        for b in &mut self.banks {
            b.fill(0);
        }
        self.bank_cfgs.fill(BankConfig::default());
        self.bank_map = BankMap::new(bank_num());
        self.bank_scoreboard.reset();
        self.deferred_bank_frees.clear();
        for bank in &mut self.mmio_banks {
            bank.fill(0);
        }
        self.total_lat = 0;
        self.npu_instruction_id = 0;
        self.matrix_instruction_count = 0;
    }

    // rushB never exposes guest DRAM. DMA commands use the host pointers
    // supplied by the native lowering, so allocating BEMU's 1 GiB guest RAM
    // would only add startup cost.
    fn new_host() -> Self {
        Self {
            memory: GuestMemory::Owned(Vec::new()),
            banks: vec![vec![0; bank_size()]; bank_num()],
            bank_cfgs: vec![BankConfig::default(); bank_num()],
            bank_map: BankMap::new(bank_num()),
            bank_scoreboard: inst::instruction::BankScoreboard::new(),
            deferred_bank_frees: Vec::new(),
            mmio_banks: vec![vec![0; mmio_bank_size()]; mmio_bank_num()],
            total_lat: 0,
            npu_instruction_id: 0,
            matrix_instruction_count: 0,
            uart: Uart::new(),
            syscall: SyscallState::new(),
            pk_vm: None,
            trace: TraceState::default(),
            profile: BemuProfile::new(false),
            barrier_hit: false,
        }
    }
}

enum HostCommand {
    Execute {
        funct7: u32,
        xs1: u64,
        xs2: u64,
        reply: mpsc::Sender<u64>,
    },
    Mvin {
        xs1: u64,
        packed_xs2: u64,
        host_ptr: usize,
        reply: mpsc::Sender<()>,
    },
    Mvout {
        xs1: u64,
        packed_xs2: u64,
        host_ptr: usize,
        reply: mpsc::Sender<()>,
    },
    Cycles {
        reply: mpsc::Sender<u64>,
    },
    Shutdown,
}

struct HostAccelerator {
    chip_id: i32,
    commands: mpsc::Sender<HostCommand>,
    worker: thread::JoinHandle<()>,
}

struct HostState {
    accelerators: HashMap<u32, HostAccelerator>,
}

static HOST_STATE: once_cell::sync::Lazy<Mutex<Option<HostState>>> = once_cell::sync::Lazy::new(|| Mutex::new(None));

thread_local! {
    // Selection belongs to the host caller. A global selected Core would race
    // when two host graph workers call the native ABI concurrently.
    static HOST_SELECTION: Cell<(u32, i32)> = const { Cell::new((0, 0)) };
}

fn spawn_host_accelerator(accelerator_id: u32, chip_id: i32) -> HostAccelerator {
    assert_eq!(chip_id, 0, "rushB BEMU supports tile chip_id 0 only");
    let (commands, receiver) = mpsc::channel();
    let worker = thread::Builder::new()
        .name(format!("rushb-bemu-core-{accelerator_id}"))
        .spawn(move || {
            let mut state = EmuState::new_host();
            while let Ok(command) = receiver.recv() {
                match command {
                    HostCommand::Execute {
                        funct7,
                        xs1,
                        xs2,
                        reply,
                    } => {
                        let _ = reply.send(host_execute(&mut state, funct7, xs1, xs2));
                    }
                    HostCommand::Mvin {
                        xs1,
                        packed_xs2,
                        host_ptr,
                        reply,
                    } => {
                        state.total_lat += inst::decode::cycles_after_issue(FUNCT7_MVIN, xs1, packed_xs2);
                        host_mvin(&mut state, xs1, packed_xs2, host_ptr as *const u8);
                        let _ = reply.send(());
                    }
                    HostCommand::Mvout {
                        xs1,
                        packed_xs2,
                        host_ptr,
                        reply,
                    } => {
                        state.total_lat += inst::decode::cycles_after_issue(FUNCT7_MVOUT, xs1, packed_xs2);
                        host_mvout(&mut state, xs1, packed_xs2, host_ptr as *mut u8);
                        let _ = reply.send(());
                    }
                    HostCommand::Cycles { reply } => {
                        let _ = reply.send(state.total_lat);
                    }
                    HostCommand::Shutdown => {
                        eprintln!(
                            "[INFO] rushB BEMU Core {accelerator_id}: instructions={} matrix={} cycles={}",
                            state.npu_instruction_id, state.matrix_instruction_count, state.total_lat
                        );
                        break;
                    }
                }
            }
        })
        .expect("failed to start rushB BEMU Core worker");
    HostAccelerator {
        chip_id,
        commands,
        worker,
    }
}

fn with_selected_accelerator<R>(f: impl FnOnce(&mpsc::Sender<HostCommand>) -> R) -> R {
    let (accelerator_id, chip_id) = HOST_SELECTION.with(Cell::get);
    let mut guard = HOST_STATE.lock().expect("rushB BEMU state poisoned");
    let state = guard.as_mut().expect("rushB is not initialized; call rushb_init first");
    let accelerator = state
        .accelerators
        .entry(accelerator_id)
        .or_insert_with(|| spawn_host_accelerator(accelerator_id, chip_id));
    assert_eq!(accelerator.chip_id, chip_id, "rushB Core cannot move between chips");
    f(&accelerator.commands)
}

fn consumes_npu_instruction_id(funct7: u32) -> bool {
    !matches!(funct7, 0 | 1)
}

fn host_execute(state: &mut EmuState, funct7: u32, xs1: u64, xs2: u64) -> u64 {
    state.barrier_hit = false;
    state.total_lat += inst::decode::cycles_after_issue(funct7, xs1, xs2);
    if consumes_npu_instruction_id(funct7) {
        state.npu_instruction_id = state.npu_instruction_id.wrapping_add(1);
    }
    if matches!(
        crate::config::ball_domain::mnemonic_for_funct(funct7).as_deref(),
        Some("SMATMUL_OS" | "SMATMUL_WS")
    ) {
        state.matrix_instruction_count = state.matrix_instruction_count.wrapping_add(1);
    }
    let instruction_id = state.npu_instruction_id;
    state.bank_scoreboard.issue(instruction_id);
    let mut ctx = inst::instruction::ExecContext {
        memory: &mut state.memory,
        banks: inst::instruction::TrackedBanks::new(&mut state.banks, Some(&state.bank_scoreboard), instruction_id),
        cfgs: &mut state.bank_cfgs,
        bank_map: &mut state.bank_map,
        deferred_bank_frees: &mut state.deferred_bank_frees,
        mmio_banks: &mut state.mmio_banks,
        barrier_hit: &mut state.barrier_hit,
    };
    let result =
        inst::decode::execute_known(funct7, xs1, xs2, &mut ctx).unwrap_or_else(|| panic!("unknown funct7: {funct7}"));
    drop(ctx);
    state.bank_scoreboard.complete(instruction_id);
    finish_deferred_bank_frees(
        &mut state.bank_cfgs,
        &mut state.bank_map,
        &mut state.deferred_bank_frees,
    );
    result
}

fn finish_deferred_bank_frees(
    bank_cfgs: &mut [BankConfig],
    bank_map: &mut BankMap,
    deferred_bank_frees: &mut Vec<u32>,
) {
    for bank_id in deferred_bank_frees.drain(..) {
        let index = bank_id as usize;
        assert!(index < bank_cfgs.len(), "deferred free: invalid bank_id {bank_id}");
        bank_map.delete_vbank(bank_id);
        bank_cfgs[index] = BankConfig::default();
    }
}

fn host_mvin(state: &mut EmuState, xs1: u64, packed_xs2: u64, host_ptr: *const u8) {
    use crate::inst::decode::{pbank, pbank_group, rs1_b0, rs1_iter, xs2_mem_stride};

    assert!(!host_ptr.is_null(), "mvin: null host pointer");
    let bank_id = rs1_b0(xs1);
    let depth = rs1_iter(xs1);
    let (_, stride) = xs2_mem_stride(packed_xs2);
    assert!(bank_id < bank_num() as u64, "mvin: invalid bank_id {bank_id}");
    assert!(depth > 0, "mvin: depth must be > 0");
    assert!(stride > 0, "mvin: stride must be > 0");

    let bi = bank_id as usize;
    assert!(state.bank_cfgs[bi].allocated, "mvin: bank {bank_id} not allocated");
    let cols = state.bank_cfgs[bi].cols;
    let groups = cols.max(1) as usize;

    unsafe {
        if groups > 1 {
            for row in 0..depth as usize {
                for group in 0..groups {
                    let p = pbank_group(&state.bank_map, bank_id, group as u64);
                    let bank_offset = row * 16;
                    assert!(bank_offset + 16 <= bank_size(), "mvin: bank range");
                    let offset = row * groups * 16 * stride as usize + group * 16;
                    state.banks[p][bank_offset..bank_offset + 16]
                        .copy_from_slice(slice::from_raw_parts(host_ptr.add(offset), 16));
                }
            }
        } else {
            let p = pbank(&state.bank_map, bank_id);
            let matrix_mode_acc = cols == 4 && depth <= MATRIX_SIZE as u64;
            let line_bytes = if matrix_mode_acc { 64usize } else { 16usize };
            for row in 0..depth as usize {
                let bank_offset = row * line_bytes;
                assert!(bank_offset + line_bytes <= bank_size(), "mvin: bank range");
                let offset = row * line_bytes * stride as usize;
                state.banks[p][bank_offset..bank_offset + line_bytes]
                    .copy_from_slice(slice::from_raw_parts(host_ptr.add(offset), line_bytes));
            }
        }
    }
    state.bank_cfgs[bi].valid_rows = depth;
}

fn host_mvout(state: &mut EmuState, xs1: u64, packed_xs2: u64, host_ptr: *mut u8) {
    use crate::inst::decode::{pbank, pbank_group, rs1_b0, rs1_iter, xs2_mem_stride};

    assert!(!host_ptr.is_null(), "mvout: null host pointer");
    let bank_id = rs1_b0(xs1);
    let depth = rs1_iter(xs1);
    let (_, stride) = xs2_mem_stride(packed_xs2);
    assert!(bank_id < bank_num() as u64, "mvout: invalid bank_id {bank_id}");
    assert!(depth > 0, "mvout: depth must be > 0");
    assert!(stride > 0, "mvout: stride must be > 0");

    let bi = bank_id as usize;
    assert!(state.bank_cfgs[bi].allocated, "mvout: bank {bank_id} not allocated");
    let cols = state.bank_cfgs[bi].cols;
    let groups = cols.max(1) as usize;

    unsafe {
        if groups > 1 {
            for row in 0..depth as usize {
                for group in 0..groups {
                    let p = pbank_group(&state.bank_map, bank_id, group as u64);
                    let bank_offset = row * 16;
                    assert!(bank_offset + 16 <= bank_size(), "mvout: bank range");
                    let offset = row * groups * 16 * stride as usize + group * 16;
                    slice::from_raw_parts_mut(host_ptr.add(offset), 16)
                        .copy_from_slice(&state.banks[p][bank_offset..bank_offset + 16]);
                }
            }
        } else {
            let p = pbank(&state.bank_map, bank_id);
            let matrix_mode_acc = cols == 4 && depth <= MATRIX_SIZE as u64;
            let line_bytes = if matrix_mode_acc { 64usize } else { 16usize };
            for row in 0..depth as usize {
                let bank_offset = row * line_bytes;
                assert!(bank_offset + line_bytes <= bank_size(), "mvout: bank range");
                let offset = row * line_bytes * stride as usize;
                slice::from_raw_parts_mut(host_ptr.add(offset), line_bytes)
                    .copy_from_slice(&state.banks[p][bank_offset..bank_offset + line_bytes]);
            }
        }
    }
}

#[cfg_attr(not(feature = "difftest"), no_mangle)]
pub extern "C" fn rushb_init() {
    let mut guard = HOST_STATE.lock().expect("rushB BEMU state poisoned");
    assert!(guard.is_none(), "rushB BEMU is already initialized");
    *guard = Some(HostState {
        accelerators: HashMap::new(),
    });
    HOST_SELECTION.with(|selection| selection.set((0, 0)));
}

#[cfg_attr(not(feature = "difftest"), no_mangle)]
pub extern "C" fn rushb_select_accelerator(accelerator_id: u32, chip_id: i32) {
    assert_eq!(chip_id, 0, "rushB BEMU supports tile chip_id 0 only");
    HOST_SELECTION.with(|selection| selection.set((accelerator_id, chip_id)));
    with_selected_accelerator(|_| {});
}

#[cfg_attr(not(feature = "difftest"), no_mangle)]
pub extern "C" fn rushb_destroy() {
    let mut guard = HOST_STATE.lock().expect("rushB BEMU state poisoned");
    if let Some(state) = guard.take() {
        for accelerator in state.accelerators.into_values() {
            let _ = accelerator.commands.send(HostCommand::Shutdown);
            accelerator.worker.join().expect("rushB BEMU Core worker panicked");
        }
    }
}

#[cfg_attr(not(feature = "difftest"), no_mangle)]
pub extern "C" fn rushb_mset(xs1: u64, xs2: u64) {
    with_selected_accelerator(|commands| {
        let (reply, result) = mpsc::channel();
        commands
            .send(HostCommand::Execute {
                funct7: FUNCT7_MSET,
                xs1,
                xs2,
                reply,
            })
            .expect("rushB BEMU Core worker stopped");
        result.recv().expect("rushB BEMU Core worker stopped");
    });
}

#[cfg_attr(not(feature = "difftest"), no_mangle)]
pub extern "C" fn rushb_mvin(xs1: u64, packed_xs2: u64, host_ptr: *const c_void) {
    with_selected_accelerator(|commands| {
        let (reply, result) = mpsc::channel();
        commands
            .send(HostCommand::Mvin {
                xs1,
                packed_xs2,
                host_ptr: host_ptr as usize,
                reply,
            })
            .expect("rushB BEMU Core worker stopped");
        result.recv().expect("rushB BEMU Core worker stopped");
    });
}

#[cfg_attr(not(feature = "difftest"), no_mangle)]
pub extern "C" fn rushb_mvout(xs1: u64, packed_xs2: u64, host_ptr: *mut c_void) {
    with_selected_accelerator(|commands| {
        let (reply, result) = mpsc::channel();
        commands
            .send(HostCommand::Mvout {
                xs1,
                packed_xs2,
                host_ptr: host_ptr as usize,
                reply,
            })
            .expect("rushB BEMU Core worker stopped");
        result.recv().expect("rushB BEMU Core worker stopped");
    });
}

#[cfg_attr(not(feature = "difftest"), no_mangle)]
pub extern "C" fn rushb_custom(xs1: u64, xs2: u64, funct7: u32) {
    with_selected_accelerator(|commands| {
        let (reply, result) = mpsc::channel();
        commands
            .send(HostCommand::Execute {
                funct7,
                xs1,
                xs2,
                reply,
            })
            .expect("rushB BEMU Core worker stopped");
        result.recv().expect("rushB BEMU Core worker stopped");
    });
}

#[cfg_attr(not(feature = "difftest"), no_mangle)]
pub extern "C" fn rushb_cycles() -> u64 {
    let guard = HOST_STATE.lock().expect("rushB BEMU state poisoned");
    let state = guard.as_ref().expect("rushB is not initialized; call rushb_init first");
    state
        .accelerators
        .values()
        .map(|accelerator| {
            let (reply, result) = mpsc::channel();
            accelerator
                .commands
                .send(HostCommand::Cycles { reply })
                .expect("rushB BEMU Core worker stopped");
            result.recv().expect("rushB BEMU Core worker stopped")
        })
        .max()
        .unwrap_or(0)
}

#[derive(Clone, Copy)]
struct GuestMap {
    virt: u64,
    phys: u64,
    len: u64,
}

struct PkVm {
    root: u64,
    next_pt: u64,
    pt_end: u64,
    next_page: u64,
    page_end: u64,
    free_pages: Vec<(u64, u64)>,
    maps: Vec<GuestMap>,
}

impl PkVm {
    fn new(memory: &mut [u8], root: u64, pt_end: u64, next_page: u64, page_end: u64) -> Result<Self, String> {
        let mut vm = Self {
            root,
            next_pt: root,
            pt_end,
            next_page,
            page_end,
            free_pages: Vec::new(),
            maps: Vec::new(),
        };
        vm.alloc_table(memory)?;
        Ok(vm)
    }

    fn satp(&self) -> u64 {
        (8u64 << 60) | ((self.root >> 12) & 0x0000_0fff_ffff_ffff)
    }

    fn map_range(&mut self, memory: &mut [u8], virt: u64, phys: u64, len: u64, flags: u64) -> Result<(), String> {
        if len == 0 {
            return Ok(());
        }
        let virt_start = align_down(virt, PAGE_SIZE);
        let phys_start = align_down(phys, PAGE_SIZE);
        let virt_end = align_up(virt + len, PAGE_SIZE);
        let mut vaddr = virt_start;
        let mut paddr = phys_start;
        while vaddr < virt_end {
            self.map_page(memory, vaddr, paddr, flags)?;
            vaddr += PAGE_SIZE;
            paddr += PAGE_SIZE;
        }
        self.maps.push(GuestMap {
            virt: virt_start,
            phys: phys_start,
            len: virt_end - virt_start,
        });
        add_guest_mapping(virt_start, phys_start, virt_end - virt_start);
        Ok(())
    }

    fn alloc_user_pages(&mut self, memory: &mut [u8], virt: u64, len: u64, flags: u64) -> Result<u64, String> {
        let size = align_up(len, PAGE_SIZE);
        let phys = match self.take_free_pages(size) {
            Some(phys) => phys,
            None => {
                let phys = self.next_page;
                self.next_page = self
                    .next_page
                    .checked_add(size)
                    .ok_or_else(|| "pk physical page allocator overflow".to_string())?;
                if self.next_page > self.page_end {
                    return Err("pk user page reserve exhausted".to_string());
                }
                phys
            }
        };
        let off = guest_offset(memory, phys)?;
        let end = off + size as usize;
        if end > memory.len() {
            return Err(format!(
                "pk physical page allocator exceeds memory: addr=0x{phys:x} size={size}"
            ));
        }
        memory[off..end].fill(0);
        self.map_range(memory, virt, phys, size, flags)?;
        Ok(phys)
    }

    fn free_user_pages(&mut self, virt: u64, len: u64) -> Result<(), String> {
        let size = align_up(len, PAGE_SIZE);
        let phys = self
            .virt_to_phys(virt, size)
            .ok_or_else(|| "pk munmap range is not mapped".to_string())?;
        self.free_pages.push((phys, size));
        Ok(())
    }

    fn take_free_pages(&mut self, size: u64) -> Option<u64> {
        let index = self.free_pages.iter().position(|(_, len)| *len >= size)?;
        let (phys, len) = self.free_pages[index];
        if len == size {
            self.free_pages.swap_remove(index);
        } else {
            self.free_pages[index] = (phys + size, len - size);
        }
        Some(phys)
    }

    fn write_user(&self, memory: &mut [u8], virt: u64, bytes: &[u8]) -> Result<(), String> {
        let phys = self
            .virt_to_phys(virt, bytes.len() as u64)
            .ok_or_else(|| format!("user write to unmapped VA: addr=0x{virt:x} size={}", bytes.len()))?;
        write_guest(memory, phys, bytes)
    }

    fn virt_to_phys(&self, virt: u64, len: u64) -> Option<u64> {
        let end = virt.checked_add(len)?;
        for map in self.maps.iter().rev() {
            let map_end = map.virt.checked_add(map.len)?;
            if virt >= map.virt && end <= map_end {
                return map.phys.checked_add(virt - map.virt);
            }
        }
        None
    }

    fn map_page(&mut self, memory: &mut [u8], virt: u64, phys: u64, flags: u64) -> Result<(), String> {
        let vpn = [(virt >> 12) & 0x1ff, (virt >> 21) & 0x1ff, (virt >> 30) & 0x1ff];
        let l2 = self.ensure_table(memory, self.root, vpn[2])?;
        let l1 = self.ensure_table(memory, l2, vpn[1])?;
        let leaf = ((phys >> 12) << 10) | flags | 0x1 | 0x10 | 0x40 | 0x80;
        self.write_pte(memory, l1, vpn[0], leaf)
    }

    fn ensure_table(&mut self, memory: &mut [u8], table: u64, idx: u64) -> Result<u64, String> {
        let pte = self.read_pte(memory, table, idx)?;
        if pte & 0x1 != 0 {
            return Ok(((pte >> 10) << 12) & !0xfffu64);
        }
        let child = self.alloc_table(memory)?;
        self.write_pte(memory, table, idx, ((child >> 12) << 10) | 0x1)?;
        Ok(child)
    }

    fn alloc_table(&mut self, memory: &mut [u8]) -> Result<u64, String> {
        let table = self.next_pt;
        self.next_pt = self
            .next_pt
            .checked_add(PAGE_SIZE)
            .ok_or_else(|| "pk page table allocator overflow".to_string())?;
        if self.next_pt > self.pt_end {
            return Err("pk page table reserve exhausted".to_string());
        }
        let off = guest_offset(memory, table)?;
        memory[off..off + PAGE_SIZE as usize].fill(0);
        Ok(table)
    }

    fn read_pte(&self, memory: &[u8], table: u64, idx: u64) -> Result<u64, String> {
        let off = guest_offset(memory, table + idx * 8)?;
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&memory[off..off + 8]);
        Ok(u64::from_le_bytes(bytes))
    }

    fn write_pte(&self, memory: &mut [u8], table: u64, idx: u64, value: u64) -> Result<(), String> {
        write_guest(memory, table + idx * 8, &value.to_le_bytes())
    }
}

unsafe fn state_mut<'a>(state: *mut c_void) -> &'a mut EmuState {
    assert!(!state.is_null(), "null BEMU state pointer");
    &mut *(state as *mut EmuState)
}

#[no_mangle]
pub extern "C" fn buckyball_init(_state: *mut c_void) {}

#[no_mangle]
pub extern "C" fn buckyball_reset(state: *mut c_void) {
    unsafe { state_mut(state) }.reset_accel();
}

#[no_mangle]
pub extern "C" fn buckyball_exec(state: *mut c_void, funct7: u8, xs1: u64, xs2: u64, pc: u64) -> u64 {
    let state = unsafe { state_mut(state) };
    state.barrier_hit = false;
    let profile_started = state.profile.begin_npu();
    let lat = inst::decode::cycles_after_issue(funct7 as u32, xs1, xs2);
    state.total_lat += lat;
    state.trace.set_bemu_clk(state.total_lat);
    if consumes_npu_instruction_id(funct7 as u32) {
        state.npu_instruction_id = state.npu_instruction_id.wrapping_add(1);
    }
    let instruction_id = state.npu_instruction_id;
    let trace = &mut state.trace as *mut TraceState;
    let btrace = state.trace.btrace_enabled();
    if btrace {
        state.bank_scoreboard.issue(instruction_id);
    }

    unsafe {
        with_trace_ptr(trace, || {
            crate::trace::itrace(crate::trace::ITraceEvent {
                funct: funct7 as u32,
                pc,
                rs1: xs1,
                rs2: xs2,
            });
        })
    };

    let EmuState {
        memory,
        banks,
        bank_cfgs,
        bank_map,
        bank_scoreboard,
        deferred_bank_frees,
        mmio_banks,
        barrier_hit,
        uart: _,
        syscall: _,
        pk_vm: _,
        trace: _,
        ..
    } = state;

    let result = unsafe {
        with_trace_ptr(trace, || {
            let mut ctx = inst::instruction::ExecContext {
                memory,
                banks: inst::instruction::TrackedBanks::new(banks, btrace.then_some(&*bank_scoreboard), instruction_id),
                cfgs: bank_cfgs,
                bank_map,
                deferred_bank_frees,
                mmio_banks,
                barrier_hit,
            };

            inst::decode::execute_known(funct7 as u32, xs1, xs2, &mut ctx)
                .unwrap_or_else(|| panic!("unknown funct7: {}", funct7))
        })
    };

    if btrace {
        let written_banks = bank_scoreboard.complete(instruction_id);
        let op_type = format!("funct7_{}", funct7);
        unsafe {
            with_trace_ptr(trace, || {
                for physical_bank_id in written_banks {
                    let (vbank_id, group_id) = bank_map
                        .logical_id(physical_bank_id)
                        .unwrap_or_else(|| panic!("BEMU wrote unmapped physical bank {physical_bank_id}"));
                    let digest = bank_hash(&banks[physical_bank_id]);
                    crate::trace::bemu_bank_digest(
                        instruction_id,
                        vbank_id,
                        group_id,
                        physical_bank_id as u32,
                        funct7 as u32,
                        &op_type,
                        digest,
                        pc,
                    );
                }
            })
        };
    }
    finish_deferred_bank_frees(bank_cfgs, bank_map, deferred_bank_frees);
    state.profile.end_npu(funct7, profile_started);

    result
}

#[no_mangle]
pub extern "C" fn bemu_barrier_hit(state: *mut c_void) -> bool {
    unsafe { (&*(state as *const EmuState)).barrier_hit }
}

/// Handle system call from guest program
/// Returns (result, should_exit)
#[no_mangle]
pub extern "C" fn handle_syscall_ffi(
    state: *mut c_void,
    syscall_num: u64,
    a0: u64,
    a1: u64,
    a2: u64,
    a3: u64,
    a4: u64,
    a5: u64,
) -> u64 {
    let state = unsafe { state_mut(state) };
    let old_brk = state.syscall.brk_addr;
    let (result, _should_exit) = handle_syscall_with_state(
        &mut state.syscall,
        syscall_num,
        a0,
        a1,
        a2,
        a3,
        a4,
        a5,
        &mut state.memory,
    );
    if let Some(mut pk_vm) = state.pk_vm.take() {
        let map_result = map_syscall_result(&mut state.memory, &mut pk_vm, old_brk, syscall_num, a0, a1, result);
        state.pk_vm = Some(pk_vm);
        if let Err(e) = map_result {
            eprintln!("[ERROR] pk syscall mapping failed: {e}");
            state.syscall.exit_code = Some(1);
            return u64::MAX;
        }
    }
    result
}

fn map_syscall_result(
    memory: &mut [u8],
    pk_vm: &mut PkVm,
    old_brk: u64,
    syscall_num: u64,
    a0: u64,
    a1: u64,
    result: u64,
) -> Result<(), String> {
    if (result as i64) < 0 {
        return Ok(());
    }

    match syscall_num {
        SYS_BRK if result > old_brk => {
            let start = align_up(old_brk, PAGE_SIZE);
            let end = align_up(result, PAGE_SIZE);
            if end > start {
                pk_vm.alloc_user_pages(memory, start, end - start, 0x2 | 0x4)?;
            }
        }
        SYS_MMAP => {
            let len = align_up(a1, PAGE_SIZE);
            if result != 0 && len != 0 {
                pk_vm.alloc_user_pages(memory, result, len, 0x2 | 0x4)?;
            }
        }
        SYS_MUNMAP => pk_vm.free_user_pages(a0, a1)?,
        _ => {
            let _ = a0;
        }
    }
    Ok(())
}

/// Check if program should exit
#[no_mangle]
pub extern "C" fn should_exit_ffi(state: *mut c_void) -> bool {
    unsafe { state_mut(state) }.syscall.exit_code.is_some()
}

/// Get exit code
#[no_mangle]
pub extern "C" fn get_exit_code_ffi(state: *mut c_void) -> i32 {
    unsafe { state_mut(state) }.syscall.exit_code.unwrap_or(0)
}

/// Handle UART MMIO load
/// IMPORTANT: uart_ptr is passed from spike_create_raw to avoid deadlock
#[no_mangle]
pub extern "C" fn uart_mmio_load(uart_ptr: *mut u8, addr: u64, size: usize) -> u64 {
    let uart = unsafe { &mut *(uart_ptr as *mut Uart) };
    let offset = addr - UART_BASE;
    uart.mmio_load(offset, size).unwrap_or(0)
}

/// Handle UART MMIO store
/// IMPORTANT: uart_ptr is passed from spike_create_raw to avoid deadlock
#[no_mangle]
pub extern "C" fn uart_mmio_store(uart_ptr: *mut u8, addr: u64, size: usize, value: u64) -> bool {
    let uart = unsafe { &mut *(uart_ptr as *mut Uart) };
    let offset = addr - UART_BASE;
    uart.mmio_store(offset, size, value)
}

extern "C" {
    fn spike_create_raw(
        isa: *const c_char,
        procs: usize,
        hart_id: usize,
        mem_ptr: *mut u8,
        mem_size: usize,
        log_path: *const c_char,
        uart_ptr: *mut u8,
        emu_state: *mut c_void,
        profile: bool,
    ) -> *mut c_void;
    fn spike_init_hart_raw(
        ctx: *mut c_void,
        entry: u64,
        trap_handler_addr: u64,
        satp: u64,
        initial_sp: u64,
        initial_a0: u64,
        initial_a1: u64,
        initial_a2: u64,
        tp_value: *const u64,
        pk: bool,
    ) -> bool;
    fn spike_step_raw(ctx: *mut c_void) -> i32;
    fn spike_finished_raw(ctx: *mut c_void) -> bool;
    fn spike_exit_code_raw(ctx: *mut c_void) -> i32;
    fn spike_stop_raw(ctx: *mut c_void, code: i32);
    fn spike_step_elapsed_ns_raw(ctx: *mut c_void) -> u64;
    fn spike_destroy_raw(ctx: *mut c_void);

}

pub struct NativeSpike {
    ctx: *mut c_void,
    state: Box<EmuState>,
    loaded_elf: Option<LoadInfo>,
}

unsafe impl Send for NativeSpike {}

impl NativeSpike {
    pub fn load_elf(&mut self, elf_path: &str) -> Result<(), String> {
        self.loaded_elf = Some(load_elf_memory(&mut self.state, elf_path)?);
        Ok(())
    }

    pub fn init_hart(&mut self, mem_mb: usize, pk: bool) -> Result<(), String> {
        let load = self
            .loaded_elf
            .take()
            .ok_or_else(|| "cannot initialize hart before loading ELF".to_string())?;
        hart_init(self.ctx, &mut self.state, load, mem_mb, pk)
    }

    pub fn step(&mut self) -> Result<(), String> {
        let ret = unsafe { spike_step_raw(self.ctx) };
        if ret < 0 {
            Err(format!("spike step failed with code {}", self.exit_code()))
        } else {
            Ok(())
        }
    }

    pub fn barrier_hit(&self) -> bool {
        bemu_barrier_hit(self.state_ptr())
    }

    fn state_ptr(&self) -> *mut c_void {
        self.state.as_ref() as *const EmuState as *mut c_void
    }

    pub fn finished(&self) -> bool {
        unsafe { spike_finished_raw(self.ctx) }
    }

    pub fn exit_code(&self) -> i32 {
        unsafe { spike_exit_code_raw(self.ctx) }
    }

    pub fn stop(&mut self, code: i32) {
        unsafe { spike_stop_raw(self.ctx, code) }
    }

    pub fn total_latency(&self) -> u64 {
        self.state.total_lat
    }

    pub fn profile_report(&self, total: Duration) -> Option<BemuProfileReport> {
        let spike_step = Duration::from_nanos(unsafe { spike_step_elapsed_ns_raw(self.ctx) });
        self.state.profile.report(total, spike_step)
    }
}

impl Drop for NativeSpike {
    fn drop(&mut self) {
        unsafe { spike_destroy_raw(self.ctx) };
    }
}

pub fn create_spike(
    isa: &str,
    hart_id: usize,
    shared_memory: Option<Arc<SharedMemory>>,
    log_path: Option<&str>,
    log_dir: &Path,
    trace_config: TraceConfig,
    profile: bool,
) -> Result<NativeSpike, String> {
    use std::ffi::CString;

    std::fs::create_dir_all(log_dir)
        .map_err(|e| format!("failed to create BEMU log dir {}: {e}", log_dir.display()))?;
    let isa_c = CString::new(isa).map_err(|e| e.to_string())?;
    let log_c = log_path.map(CString::new).transpose().map_err(|e| e.to_string())?;
    let mut state = Box::new(EmuState::new(log_dir, trace_config, profile, shared_memory)?);
    let mem_ptr = state.memory.as_mut_ptr();
    let mem_size = state.memory.len();
    let uart_ptr = &mut state.uart as *mut Uart as *mut u8;
    let state_ptr = &mut *state as *mut EmuState as *mut c_void;

    let ctx = unsafe {
        spike_create_raw(
            isa_c.as_ptr(),
            1,
            hart_id,
            mem_ptr,
            mem_size,
            log_c.as_ref().map_or(std::ptr::null(), |path| path.as_ptr()),
            uart_ptr,
            state_ptr,
            profile,
        )
    };
    if ctx.is_null() {
        Err("failed to create spike instance".to_string())
    } else {
        Ok(NativeSpike {
            ctx,
            state,
            loaded_elf: None,
        })
    }
}

struct HartInit {
    entry: u64,
    trap_handler_addr: u64,
    satp: u64,
    regs: InitialRegs,
    tp: Option<u64>,
    pk: bool,
}

fn load_elf_memory(state: &mut EmuState, elf_path: &str) -> Result<LoadInfo, String> {
    let load = load_elf(elf_path, &mut state.memory, DRAM_BASE)?;
    let entry = load.entry;
    let mem_end = DRAM_BASE + state.memory.len() as u64;

    if entry < DRAM_BASE || entry >= mem_end {
        return Err(format!(
            "ELF entry outside BEMU DRAM: original=0x{:x} entry=0x{:x} valid=0x{:x}..0x{:x}",
            load.analysis.original_entry, entry, DRAM_BASE, mem_end
        ));
    }

    if load.analysis.needs_relocation {
        eprintln!(
            "[INFO] relocated ELF: entry 0x{:x} -> 0x{:x}, image 0x{:x}..0x{:x} -> end 0x{:x}",
            load.analysis.original_entry,
            load.analysis.entry,
            load.analysis.min_vaddr,
            load.analysis.max_vaddr,
            load.analysis.image_end
        );
    }

    Ok(load)
}

fn hart_init(ctx: *mut c_void, state: &mut EmuState, load: LoadInfo, mem_mb: usize, pk: bool) -> Result<(), String> {
    let mem_end = DRAM_BASE + state.memory.len() as u64;
    state.syscall = SyscallState::new();
    state.pk_vm = None;
    set_guest_mappings(&[]);

    let brk_start = if pk {
        align_up(load.analysis.max_vaddr, PAGE_SIZE)
    } else {
        align_up(load.image_end, PAGE_SIZE)
    };
    let mmap_base = if pk {
        align_down(PK_MMAP_CEILING, PAGE_SIZE)
    } else {
        align_down(mem_end - 8 * 1024 * 1024, PAGE_SIZE)
    };
    state.syscall.init_mem_layout(brk_start, mmap_base);
    if pk {
        state
            .syscall
            .set_mem_bounds(load.analysis.min_vaddr, USER_TOP - USER_STACK_SIZE);
    }

    let tp = if pk {
        None
    } else {
        setup_tls(&mut state.memory, load.tls)?
    };
    let dtb_addr = install_dtb(&mut state.memory, mem_mb)?;

    let trap_handler_addr = if pk {
        install_pk_trap_handler(&mut state.memory)?
    } else {
        0
    };
    let (entry, satp, initial_regs) = if pk {
        let pk_vm = setup_pk_vm(&mut state.memory, &load)?;
        let regs = setup_pk_stack(&mut state.memory, &pk_vm, &load)?;
        let satp = pk_vm.satp();
        state.pk_vm = Some(pk_vm);
        (load.analysis.original_entry, satp, regs)
    } else {
        (
            load.entry,
            0,
            InitialRegs {
                sp: 0,
                a0: 0,
                a1: dtb_addr,
                a2: 0,
            },
        )
    };

    let hart = HartInit {
        entry,
        trap_handler_addr,
        satp,
        regs: initial_regs,
        tp,
        pk,
    };
    let tp_ptr = hart.tp.as_ref().map(|v| v as *const u64).unwrap_or(std::ptr::null());

    let initialized = unsafe {
        spike_init_hart_raw(
            ctx,
            hart.entry,
            hart.trap_handler_addr,
            hart.satp,
            hart.regs.sp,
            hart.regs.a0,
            hart.regs.a1,
            hart.regs.a2,
            tp_ptr,
            hart.pk,
        )
    };
    if initialized {
        Ok(())
    } else {
        Err("failed to initialize Spike hart state".to_string())
    }
}

struct InitialRegs {
    sp: u64,
    a0: u64,
    a1: u64,
    a2: u64,
}

fn setup_tls(memory: &mut [u8], tls: Option<TlsInfo>) -> Result<Option<u64>, String> {
    let Some(tls) = tls else {
        return Ok(None);
    };

    let align = tls.align.max(16);
    let tls_size = align_up(tls.memsz, align);
    let total_size = tls_size + align;
    let tls_area_addr = DRAM_BASE + memory.len() as u64 - total_size - 0x10000;
    let tp = align_up(tls_area_addr, align);
    let copy_size = tls.filesz.min(tls.memsz) as usize;

    if copy_size > 0 {
        let src_offset = guest_offset(memory, tls.vaddr)?;
        let dst_offset = guest_offset(memory, tp)?;
        let src = memory
            .get(src_offset..src_offset + copy_size)
            .ok_or_else(|| format!("TLS source exceeds memory: addr=0x{:x} size={copy_size}", tls.vaddr))?
            .to_vec();
        let dst = memory
            .get_mut(dst_offset..dst_offset + copy_size)
            .ok_or_else(|| format!("TLS destination exceeds memory: addr=0x{tp:x} size={copy_size}"))?;
        dst.copy_from_slice(&src);
    }

    if tls.memsz > tls.filesz {
        let bss_start = tp + tls.filesz;
        let bss_offset = guest_offset(memory, bss_start)?;
        let bss_size = (tls.memsz - tls.filesz) as usize;
        memory
            .get_mut(bss_offset..bss_offset + bss_size)
            .ok_or_else(|| format!("TLS BSS exceeds memory: addr=0x{bss_start:x} size={bss_size}"))?
            .fill(0);
    }

    write_guest(memory, tp, &tp.to_le_bytes())?;
    Ok(Some(tp))
}

fn install_dtb(memory: &mut [u8], mem_mb: usize) -> Result<u64, String> {
    let dtb = DtbBuilder::build_minimal(DRAM_BASE, mem_mb as u64 * (1 << 20), None, None);
    let mem_end = DRAM_BASE + memory.len() as u64;
    let dtb_addr = align_down(mem_end - 0x20_0000 - dtb.len() as u64, PAGE_SIZE);
    write_guest(memory, dtb_addr, &dtb)?;
    Ok(dtb_addr)
}

fn install_pk_trap_handler(memory: &mut [u8]) -> Result<u64, String> {
    let trap_handler_addr = DRAM_BASE + memory.len() as u64 - 0x2000;
    let syscall_magic_addr = DRAM_BASE + memory.len() as u64 - 0x1000;
    let offset = syscall_magic_addr as i64 - trap_handler_addr as i64;
    let imm20 = ((offset >> 20) as u32) & 0x1;
    let imm10_1 = ((offset >> 1) as u32) & 0x3ff;
    let imm11 = ((offset >> 11) as u32) & 0x1;
    let imm19_12 = ((offset >> 12) as u32) & 0xff;
    let jal = 0x6f | (imm19_12 << 12) | (imm11 << 20) | (imm10_1 << 21) | (imm20 << 31);
    write_guest(memory, trap_handler_addr, &jal.to_le_bytes())?;
    Ok(trap_handler_addr)
}

fn setup_pk_vm(memory: &mut [u8], load: &LoadInfo) -> Result<PkVm, String> {
    let mem_end = DRAM_BASE + memory.len() as u64;
    let pt_root = align_down(mem_end - PK_HIGH_RESERVE, PAGE_SIZE);
    let pt_end = pt_root + PK_PT_RESERVE;
    let stack_phys_bottom = align_down(pt_root - USER_STACK_SIZE, PAGE_SIZE);
    let stack_virt_bottom = USER_TOP - USER_STACK_SIZE;
    let next_page = align_up(load.image_end, PAGE_SIZE);
    let mut vm = PkVm::new(memory, pt_root, pt_end, next_page, stack_phys_bottom)?;

    for seg in &load.analysis.load_segments {
        let phys = if load.analysis.is_pie || load.analysis.needs_relocation {
            DRAM_BASE + (seg.vaddr - load.analysis.min_vaddr)
        } else {
            seg.vaddr
        };
        let mut flags = 0;
        if (seg.flags & 0x4) != 0 {
            flags |= 0x2;
        }
        if (seg.flags & 0x2) != 0 {
            flags |= 0x4;
        }
        if (seg.flags & 0x1) != 0 {
            flags |= 0x8;
        }
        vm.map_range(memory, seg.vaddr, phys, seg.memsz, flags)?;
    }

    vm.map_range(memory, stack_virt_bottom, stack_phys_bottom, USER_STACK_SIZE, 0x2 | 0x4)?;
    let maps: Vec<(u64, u64, u64)> = vm.maps.iter().map(|m| (m.virt, m.phys, m.len)).collect();
    set_guest_mappings(&maps);
    Ok(vm)
}

fn setup_pk_stack(memory: &mut [u8], vm: &PkVm, load: &LoadInfo) -> Result<InitialRegs, String> {
    const AT_NULL: u64 = 0;
    const AT_PHDR: u64 = 3;
    const AT_PHENT: u64 = 4;
    const AT_PHNUM: u64 = 5;
    const AT_PAGESZ: u64 = 6;
    const AT_BASE: u64 = 7;
    const AT_ENTRY: u64 = 9;
    const AT_UID: u64 = 11;
    const AT_EUID: u64 = 12;
    const AT_GID: u64 = 13;
    const AT_EGID: u64 = 14;
    const AT_HWCAP: u64 = 16;
    const AT_SECURE: u64 = 23;
    const AT_RANDOM: u64 = 25;
    const AT_HWCAP2: u64 = 26;
    const AT_EXECFN: u64 = 31;

    let stack_top = align_down(USER_TOP - 16, 16);
    let prog_name = b"tutorial-linux\0";
    let random_len = 16u64;
    let word_size = 8u64;

    let string_addr = align_down(stack_top - prog_name.len() as u64, 16);
    let random_addr = align_down(string_addr - random_len, 16);
    let phdr_addr = user_image_addr(load, load.program_headers.addr)?;

    let mut stack_entries = Vec::with_capacity(40);
    stack_entries.push(1);
    stack_entries.push(string_addr);
    stack_entries.push(0);
    stack_entries.push(0);
    stack_entries.extend_from_slice(&[
        AT_PHDR,
        phdr_addr,
        AT_PHENT,
        load.program_headers.entry_size,
        AT_PHNUM,
        load.program_headers.count,
        AT_PAGESZ,
        PAGE_SIZE,
        AT_BASE,
        0,
        AT_HWCAP,
        0,
        AT_ENTRY,
        load.analysis.original_entry,
        AT_UID,
        0,
        AT_EUID,
        0,
        AT_GID,
        0,
        AT_EGID,
        0,
        AT_SECURE,
        0,
        AT_RANDOM,
        random_addr,
        AT_HWCAP2,
        0,
        AT_EXECFN,
        string_addr,
        AT_NULL,
        0,
    ]);

    let sp = align_down(random_addr - stack_entries.len() as u64 * word_size, 16);
    vm.write_user(memory, string_addr, prog_name)?;
    for i in 0..random_len {
        vm.write_user(memory, random_addr + i, &[0xA5u8 ^ i as u8])?;
    }
    for (i, value) in stack_entries.iter().enumerate() {
        vm.write_user(memory, sp + i as u64 * word_size, &value.to_le_bytes())?;
    }

    Ok(InitialRegs {
        sp,
        a0: 0,
        a1: 0,
        a2: 0,
    })
}

fn user_image_addr(load: &LoadInfo, phys_addr: u64) -> Result<u64, String> {
    if !load.analysis.is_pie && !load.analysis.needs_relocation {
        return Ok(phys_addr);
    }
    if phys_addr < DRAM_BASE || phys_addr > load.image_end {
        return Err(format!("loaded image address outside relocated image: 0x{phys_addr:x}"));
    }
    Ok(load.analysis.min_vaddr + (phys_addr - DRAM_BASE))
}

fn write_guest(memory: &mut [u8], addr: u64, bytes: &[u8]) -> Result<(), String> {
    let offset = guest_offset(memory, addr)?;
    let end = offset + bytes.len();
    if end > memory.len() {
        return Err(format!(
            "guest write exceeds memory: addr=0x{addr:x} size={}",
            bytes.len()
        ));
    }
    memory[offset..end].copy_from_slice(bytes);
    Ok(())
}

fn guest_offset(memory: &[u8], addr: u64) -> Result<usize, String> {
    if addr < DRAM_BASE {
        return Err(format!("guest address below DRAM: 0x{addr:x}"));
    }
    let offset = (addr - DRAM_BASE) as usize;
    if offset >= memory.len() {
        return Err(format!("guest address outside memory: 0x{addr:x}"));
    }
    Ok(offset)
}

fn align_down(value: u64, align: u64) -> u64 {
    value & !(align - 1)
}

fn align_up(value: u64, align: u64) -> u64 {
    (value + align - 1) & !(align - 1)
}
