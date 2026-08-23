//===- 35_mvin_mmio.rs - MVIN_MMIO instruction (DRAM to MMIO) --------------===//
//
// DMA-load metadata from DRAM into MMIO SRAM.
// DMA transfers full 128-bit rows (16 bytes each); `col` controls the per-row
// byte mask written into MMIO (first `col` bytes written, rest zero-masked).
//
// rs1[9:0]:    0 (no main bank dependency)
// rs1[63:30]:  row (BB_ITER, row count for DMA beats)
// rs2[38:0]:   dram_addr (39-bit DRAM virtual address)
// rs2[55:39]:  mmio_addr (17-bit MMIO byte address)
// rs2[63:56]:  col (8-bit, valid bytes per row, 1..16)
//
//===-----------------------------------------------------------------===//-----===//

use super::super::bank::{bank_row_bytes, mem_read, mmio_bank_num, mmio_enable, mmio_total_size};
use super::instruction::{ExecContext, Instruction};

pub struct MvinMmio;

impl Instruction for MvinMmio {
    const FUNCT: u32 = 35;

    fn exec(xs1: u64, xs2: u64, ctx: &mut ExecContext) -> u64 {
        if !mmio_enable() {
            panic!("mvin_mmio: MMIO is disabled for this BEMU chip config");
        }

        let dram_addr = xs2 & 0x7F_FFFF_FFFF; // bits [38:0]
        let mmio_addr = ((xs2 >> 39) & 0x1_FFFF) as u32; // bits [55:39], 17-bit
        let col = ((xs2 >> 56) & 0xFF) as u8; // bits [63:56]
        let row = ((xs1 >> 30) & 0x3_FFFF_FFFF) as u32; // bits [63:30], 34-bit

        let bytes_per_row = bank_row_bytes();
        for r in 0..row as usize {
            let src_addr = dram_addr + (r * bytes_per_row) as u64;
            let dst_offset = mmio_addr as usize + r * bytes_per_row;

            if dst_offset + bytes_per_row > mmio_total_size() {
                panic!("mvin_mmio: MMIO address out of range");
            }

            for b in 0..bytes_per_row {
                let abs_addr = dst_offset + b;
                let bank_idx = abs_addr % mmio_bank_num();
                let bank_offset = abs_addr / mmio_bank_num();
                ctx.mmio_banks[bank_idx][bank_offset] = if b < col as usize {
                    mem_read(ctx.memory, src_addr + b as u64)
                } else {
                    0
                };
            }
        }
        0
    }

    fn latency(_xs1: u64, _xs2: u64) -> u64 {
        // row count is in xs1[63:30], not xs2
        let row = (_xs1 >> 30) & 0x3_FFFF_FFFF;
        row.max(1)
    }
}
