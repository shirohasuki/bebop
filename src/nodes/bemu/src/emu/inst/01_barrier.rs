//===- 01_barrier.rs - BARRIER instruction ---------------------------------===//

use super::instruction::{ExecContext, Instruction};

pub struct Barrier;

impl Instruction for Barrier {
    const FUNCT: u32 = 1;

    fn exec(_xs1: u64, _xs2: u64, ctx: &mut ExecContext) -> u64 {
        *ctx.barrier_hit = true;
        0
    }

    fn latency(_xs1: u64, _xs2: u64) -> u64 {
        1
    }
}
