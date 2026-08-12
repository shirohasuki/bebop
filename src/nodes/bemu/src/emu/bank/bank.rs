#[allow(unused_imports)]
pub use crate::config::{
    bank_lines, bank_num, bank_row_bytes, bank_size, bank_width, mmio_bank_lines, mmio_bank_num, mmio_bank_row_bytes,
    mmio_bank_size, mmio_bank_width, mmio_enable, mmio_read_width, mmio_total_size,
};

pub const MATRIX_SIZE: usize = 16;

/// Mirrors RTL `PrivateMemBackend.mappingTable`:
/// physical SRAM bank slot -> bound virtual bank id.
#[derive(Clone, Default, Debug)]
pub struct MapEntry {
    pub valid: bool,
    pub vbank_id: u32,
    pub group_id: u32,
}

#[derive(Clone, Debug)]
pub struct BankMap {
    pub slots: Vec<MapEntry>,
}

impl BankMap {
    pub fn new(num_physical: usize) -> Self {
        Self {
            slots: vec![MapEntry::default(); num_physical],
        }
    }

    pub fn delete_vbank(&mut self, v: u32) {
        for e in &mut self.slots {
            if e.valid && e.vbank_id == v {
                *e = MapEntry::default();
            }
        }
    }

    pub fn first_free_pbank(&self) -> Option<usize> {
        self.slots.iter().position(|e| !e.valid)
    }

    pub fn bind_group(&mut self, p: usize, v: u32, group: u32) {
        self.slots[p].valid = true;
        self.slots[p].vbank_id = v;
        self.slots[p].group_id = group;
    }

    #[allow(dead_code)]
    pub fn resolve(&self, v: u32) -> Option<usize> {
        self.resolve_group(v, 0)
    }

    pub fn resolve_group(&self, v: u32, group: u32) -> Option<usize> {
        self.slots
            .iter()
            .position(|e| e.valid && e.vbank_id == v && e.group_id == group)
    }

    /// Return the architectural identity currently bound to a physical slot.
    pub fn logical_id(&self, physical_bank_id: usize) -> Option<(u32, u32)> {
        self.slots
            .get(physical_bank_id)
            .and_then(|entry| entry.valid.then_some((entry.vbank_id, entry.group_id)))
    }
}

#[derive(Default, Clone, Copy, Debug)]
pub struct BankConfig {
    pub allocated: bool,
    pub cols: u64,
    /// Rows populated by the most recent MVIN. Gemmini uses this to apply the
    /// CISC zero-op1-tail contract without guessing from bank contents.
    pub valid_rows: u64,
}

/// DRAM is mapped at this base address from the guest's perspective.
/// Must match `DRAM_BASE` in spike.cc.
pub const DRAM_BASE: u64 = 0x80000000;

#[inline]
fn dram_offset(mem_len: usize, addr: u64) -> usize {
    if let Some(off) = bebop_syscall::translate_guest_addr(addr, 1, mem_len) {
        return off;
    }

    let mem_end = DRAM_BASE + mem_len as u64;
    if addr >= DRAM_BASE && addr < mem_end {
        return (addr - DRAM_BASE) as usize;
    }

    panic!(
        "DRAM access out of range: addr=0x{:x} (valid range 0x{:x}-0x{:x})",
        addr, DRAM_BASE, mem_end
    );
}

#[inline]
pub fn mem_read(mem: &[u8], addr: u64) -> u8 {
    mem[dram_offset(mem.len(), addr)]
}

#[inline]
pub fn mem_write(mem: &mut [u8], addr: u64, v: u8) {
    let off = dram_offset(mem.len(), addr);
    mem[off] = v;
}
