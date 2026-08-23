//===- 16_mvout.rs - MVOUT instruction (bank to memory) --------------------===//

use super::super::bank::{bank_num, bank_size, mem_write, MATRIX_SIZE};
use super::decode::{pbank, pbank_group, rs1_b0, rs1_iter, xs2_mem_stride};
use super::instruction::{ExecContext, Instruction};

pub struct Mvout;

impl Instruction for Mvout {
    const FUNCT: u32 = 16;

    fn exec(xs1: u64, xs2: u64, ctx: &mut ExecContext) -> u64 {
        let bank_id = rs1_b0(xs1);
        let depth = rs1_iter(xs1);
        let (mem_addr, stride) = xs2_mem_stride(xs2);

        if bank_id >= bank_num() as u64 {
            panic!("mvout: invalid bank_id {bank_id}");
        }

        if depth == 0 {
            panic!("mvout: depth must be > 0");
        }

        if stride == 0 {
            panic!("mvout: stride must be > 0");
        }

        let bi = bank_id as usize;
        if !ctx.cfgs[bi].allocated {
            panic!("mvout: bank {bank_id} not allocated");
        }

        let cols = ctx.cfgs[bi].cols;
        let groups = cols.max(1) as usize;

        if groups > 1 {
            // depth is virtual-bank rows (same contract as mvin groups>1).
            for i in 0..depth as usize {
                for group in 0..groups {
                    let p = pbank_group(ctx.bank_map, bank_id, group as u64);
                    let bank_offset = i * 16;
                    if bank_offset + 16 > bank_size() {
                        panic!("mvout: bank range: bank_offset={bank_offset} line_bytes=16 depth={depth}");
                    }
                    let addr = mem_addr + i as u64 * groups as u64 * 16 * stride + group as u64 * 16;
                    for j in 0..16 {
                        mem_write(ctx.memory, addr + j as u64, ctx.banks[p][bank_offset + j]);
                    }
                }
            }
            let row_stride = groups as u64 * 16 * stride;
            for group in 0..groups {
                let p = pbank_group(ctx.bank_map, bank_id, group as u64);
                crate::trace::mtrace(crate::trace::MTraceEvent {
                    is_write: true,
                    addr: mem_addr + group as u64 * 16,
                    rows: depth,
                    line_bytes: 16,
                    row_stride,
                    vbank_id: bank_id as u32,
                    pbank_id: p as u32,
                    group_id: group as u32,
                });
            }
        } else {
            let p = pbank(ctx.bank_map, bank_id);
            let matrix_mode_acc = cols == 4 && depth <= MATRIX_SIZE as u64;
            let line_bytes = if matrix_mode_acc { 64usize } else { 16usize };

            for i in 0..depth {
                let bank_offset = (i as usize) * line_bytes;
                if bank_offset + line_bytes > bank_size() {
                    panic!("mvout: bank range: bank_offset={bank_offset} line_bytes={line_bytes} depth={depth}");
                }
                let addr = mem_addr + i * line_bytes as u64 * stride;
                for j in 0..line_bytes {
                    mem_write(ctx.memory, addr + j as u64, ctx.banks[p][bank_offset + j]);
                }
            }
            crate::trace::mtrace(crate::trace::MTraceEvent {
                is_write: true,
                addr: mem_addr,
                rows: depth,
                line_bytes: line_bytes as u32,
                row_stride: line_bytes as u64 * stride,
                vbank_id: bank_id as u32,
                pbank_id: p as u32,
                group_id: 0,
            });
        }
        0
    }

    fn latency(xs1: u64, _xs2: u64) -> u64 {
        rs1_iter(xs1).max(1)
    }
}
