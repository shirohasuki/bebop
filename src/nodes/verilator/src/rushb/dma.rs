use super::state::BankConfig;
use crate::ffi::{bbsim_host_memory_range, bbsim_host_memory_read, bbsim_host_memory_write};
use std::collections::HashMap;

const DMA_ADDR_MASK: u64 = (1_u64 << 39) - 1;

#[derive(Debug)]
pub(crate) struct DmaChunk {
    pub(crate) offset: usize,
    pub(crate) data: Vec<u8>,
}

pub(crate) enum DmaOperation {
    None,
    Mvin {
        spans: Vec<(usize, usize)>,
        chunks: Vec<DmaChunk>,
    },
    Mvout {
        spans: Vec<(usize, usize)>,
    },
}

pub(crate) struct PreparedDma {
    pub(crate) address: u64,
    pub(crate) spans: Vec<(usize, usize)>,
    pub(crate) output: bool,
}

#[derive(Clone, Copy)]
struct StagingRegion {
    base: u64,
    next: u64,
}

#[derive(Default)]
pub(crate) struct StagingAllocator {
    regions: HashMap<i32, StagingRegion>,
}

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

fn staging_bytes(spans: &[(usize, usize)]) -> Result<u64, String> {
    let bytes = spans
        .iter()
        .map(|(offset, size)| offset.checked_add(*size).ok_or_else(|| "DMA span overflow".to_string()))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .max()
        .ok_or_else(|| "DMA has no spans".to_string())?;
    Ok((u64::try_from(bytes).map_err(|_| "DMA staging size too large".to_string())? + 63) & !63)
}

impl StagingAllocator {
    pub(crate) fn reset(&mut self) {
        self.regions.clear();
    }

    pub(crate) fn allocate(&mut self, chip_id: i32, spans: &[(usize, usize)]) -> Result<u64, String> {
        let bytes = staging_bytes(spans)?;
        let region = if let Some(region) = self.regions.get_mut(&chip_id) {
            region
        } else {
            let mut base = 0;
            let mut size = 0;
            unsafe {
                if !bbsim_host_memory_range(chip_id, &mut base, &mut size) {
                    return Err(format!("BBSimDRAM is not initialized for chip {chip_id}"));
                }
            }
            let end = base
                .checked_add(size)
                .ok_or_else(|| "BBSimDRAM address range overflow".to_string())?;
            self.regions.insert(chip_id, StagingRegion { base, next: end });
            self.regions.get_mut(&chip_id).expect("staging region was inserted")
        };

        let address = region
            .next
            .checked_sub(bytes)
            .ok_or_else(|| format!("DMA staging space exhausted for chip {chip_id}"))?;
        if address < region.base || address > DMA_ADDR_MASK {
            return Err(format!(
                "DMA staging space exhausted or outside ISA address width: chip={chip_id} address=0x{address:x}"
            ));
        }
        region.next = address;
        Ok(address)
    }
}

pub(crate) unsafe fn capture_host(host: *const u8, spans: &[(usize, usize)]) -> Vec<DmaChunk> {
    assert!(!host.is_null(), "mvin host pointer is null");
    spans
        .iter()
        .map(|&(offset, size)| DmaChunk {
            offset,
            data: std::slice::from_raw_parts(host.add(offset), size).to_vec(),
        })
        .collect()
}

pub(crate) unsafe fn restore_host(host: *mut u8, chunks: &[DmaChunk]) {
    assert!(!host.is_null(), "mvout host pointer is null");
    for chunk in chunks {
        std::ptr::copy_nonoverlapping(chunk.data.as_ptr(), host.add(chunk.offset), chunk.data.len());
    }
}

pub(crate) fn write_staging(chip_id: i32, address: u64, chunks: &[DmaChunk]) -> Result<(), String> {
    for chunk in chunks {
        let ok = unsafe {
            bbsim_host_memory_write(
                chip_id,
                address + chunk.offset as u64,
                chunk.data.as_ptr(),
                chunk.data.len() as u64,
            )
        };
        if !ok {
            return Err(format!(
                "mvin staging write exceeds BBSimDRAM: chip={chip_id} address=0x{:x}",
                address + chunk.offset as u64
            ));
        }
    }
    Ok(())
}

pub(crate) fn read_staging(chip_id: i32, address: u64, spans: &[(usize, usize)]) -> Result<Vec<DmaChunk>, String> {
    let mut chunks = Vec::with_capacity(spans.len());
    for &(offset, size) in spans {
        let mut data = vec![0u8; size];
        let ok = unsafe { bbsim_host_memory_read(chip_id, address + offset as u64, data.as_mut_ptr(), size as u64) };
        if !ok {
            return Err(format!(
                "mvout staging read exceeds BBSimDRAM: chip={chip_id} address=0x{:x}",
                address + offset as u64
            ));
        }
        chunks.push(DmaChunk { offset, data });
    }
    Ok(chunks)
}

pub(crate) fn staged_xs2(packed_xs2: u64, address: u64) -> u64 {
    (packed_xs2 & !DMA_ADDR_MASK) | address
}
