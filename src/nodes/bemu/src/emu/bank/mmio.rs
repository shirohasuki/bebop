//===- mmio.rs - MMIO bank read operations ---------------------------------===//
//
// Provides MMIO read functionality for Ball instructions.
// Balls can read per-element or per-block metadata (e.g., scales) from MMIO.
//
//===-----------------------------------------------------------------===//

use super::{mmio_bank_num, mmio_enable, mmio_total_size};

/// Read a byte from MMIO banks.
///
/// # Arguments
/// * `mmio_banks` - MMIO banks sized from the active chip memdomain TOML
/// * `addr` - Absolute byte address in the unified MMIO space
///
/// # Returns
/// The byte value at the specified MMIO address.
#[allow(dead_code)]
pub fn mmio_read_byte(mmio_banks: &[Vec<u8>], addr: usize) -> u8 {
    if !mmio_enable() {
        panic!("mmio_read_byte: MMIO is disabled for this BEMU chip config");
    }

    if addr >= mmio_total_size() {
        panic!("mmio_read_byte: address 0x{:x} out of range", addr);
    }

    let bank_idx = addr % mmio_bank_num();
    let bank_offset = addr / mmio_bank_num();

    mmio_banks[bank_idx][bank_offset]
}

pub fn mmio_write_byte(mmio_banks: &mut [Vec<u8>], addr: usize, data: u8) {
    if !mmio_enable() {
        panic!("mmio_write_byte: MMIO is disabled for this BEMU chip config");
    }
    if addr >= mmio_total_size() {
        panic!("mmio_write_byte: address 0x{:x} out of range", addr);
    }
    let bank_idx = addr % mmio_bank_num();
    let bank_offset = addr / mmio_bank_num();
    mmio_banks[bank_idx][bank_offset] = data;
}
