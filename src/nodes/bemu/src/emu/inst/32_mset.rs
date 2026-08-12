//===- 32_mset.rs - MSET instruction (bank allocation) ---------------------===//

use super::super::bank::{bank_num, BankConfig};
use super::decode::{rs1_b0, xs2_mset};
use super::instruction::{ExecContext, Instruction};

pub struct Mset;

impl Instruction for Mset {
    const FUNCT: u32 = 32;

    fn exec(xs1: u64, xs2: u64, ctx: &mut ExecContext) -> u64 {
        let bank_id = rs1_b0(xs1);
        let (_rows, col, alloc) = xs2_mset(xs2);

        if bank_id >= bank_num() as u64 {
            panic!("mset: invalid bank_id {bank_id}");
        }

        let v = bank_id as u32;
        let i = bank_id as usize;
        let groups = col.max(1);

        if alloc == 1 {
            ctx.bank_map.delete_vbank(v);
            for group in 0..groups {
                let p = ctx
                    .bank_map
                    .first_free_pbank()
                    .unwrap_or_else(|| panic!("mset: no free physical bank"));
                ctx.bank_map.bind_group(p, v, group as u32);
                ctx.banks.initialize(p, 0);
            }
            ctx.cfgs[i] = BankConfig {
                allocated: true,
                cols: col,
                valid_rows: 0,
            };
        } else {
            ctx.bank_map.delete_vbank(v);
            ctx.cfgs[i] = BankConfig {
                allocated: false,
                cols: 0,
                valid_rows: 0,
            };
        }
        0
    }

    fn latency(_xs1: u64, _xs2: u64) -> u64 {
        1
    }
}
