use super::state::BankConfig;
use crate::ffi::{bbsim_host_memory_range, bbsim_host_memory_read, bbsim_host_memory_write};

const DMA_ADDR_MASK: u64 = (1_u64 << 39) - 1;

pub(crate) fn spans(config: BankConfig, xs1: u64, packed_xs2: u64) -> Vec<(usize, usize)> {
    assert!(config.allocated, "DMA references an unallocated Buckyball bank");
    assert!(config.groups > 0, "DMA references a bank with unknown group count");

    let depth = xs1 >> 30;
    let stride = (packed_xs2 >> 39) & 0x7_ffff;
    assert!(depth > 0, "DMA depth must be non-zero");
    assert!(stride > 0, "DMA stride must be non-zero");

    let mut spans = Vec::new();
    if config.groups > 1 {
        for row in 0..depth {
            for group in 0..config.groups {
                let offset = row
                    .checked_mul(config.groups)
                    .and_then(|value| value.checked_mul(16))
                    .and_then(|value| value.checked_mul(stride))
                    .and_then(|value| value.checked_add(group * 16))
                    .expect("DMA address overflow");
                spans.push((usize::try_from(offset).expect("DMA offset too large"), 16));
            }
        }
    } else {
        for row in 0..depth {
            let offset = row
                .checked_mul(16)
                .and_then(|value| value.checked_mul(stride))
                .expect("DMA address overflow");
            spans.push((usize::try_from(offset).expect("DMA offset too large"), 16));
        }
    }
    spans
}

pub(crate) fn staging_address(chip_id: i32, spans: &[(usize, usize)]) -> u64 {
    let bytes = spans
        .iter()
        .map(|(offset, size)| offset.checked_add(*size).expect("DMA span overflow"))
        .max()
        .expect("DMA has no spans");
    let aligned_bytes = (u64::try_from(bytes).expect("DMA staging size too large") + 15) & !15;

    let mut base = 0;
    let mut size = 0;
    unsafe {
        assert!(
            bbsim_host_memory_range(chip_id, &mut base, &mut size),
            "BBSimDRAM is not initialized for requested chip"
        );
    }
    assert!(aligned_bytes <= size, "DMA staging buffer exceeds BBSimDRAM");
    let address = base + size - aligned_bytes;
    assert!(
        address <= DMA_ADDR_MASK,
        "BBSimDRAM staging address exceeds ISA address width"
    );
    address
}

pub(crate) unsafe fn copy_to_staging(chip_id: i32, address: u64, host: *const u8, spans: &[(usize, usize)]) {
    assert!(!host.is_null(), "mvin host pointer is null");
    for &(offset, size) in spans {
        assert!(
            bbsim_host_memory_write(chip_id, address + offset as u64, host.add(offset), size as u64),
            "mvin staging write exceeds BBSimDRAM"
        );
    }
}

pub(crate) unsafe fn copy_from_staging(chip_id: i32, address: u64, host: *mut u8, spans: &[(usize, usize)]) {
    assert!(!host.is_null(), "mvout host pointer is null");
    for &(offset, size) in spans {
        assert!(
            bbsim_host_memory_read(chip_id, address + offset as u64, host.add(offset), size as u64),
            "mvout staging read exceeds BBSimDRAM"
        );
    }
}

pub(crate) fn staged_xs2(packed_xs2: u64, address: u64) -> u64 {
    (packed_xs2 & !DMA_ADDR_MASK) | address
}
