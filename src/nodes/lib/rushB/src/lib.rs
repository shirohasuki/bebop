//! Shared Rust contract for the backend-neutral rushB host ABI.

pub const FUNCT7_FENCE: u32 = 0;
pub const FUNCT7_MVOUT: u32 = 16;
pub const FUNCT7_MSET: u32 = 32;
pub const FUNCT7_MVIN: u32 = 33;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RushBSelection {
    pub accelerator_id: u32,
    pub chip_id: i32,
}

impl RushBSelection {
    pub const fn new(accelerator_id: u32, chip_id: i32) -> Self {
        Self {
            accelerator_id,
            chip_id,
        }
    }
}

pub const DEFAULT_SELECTION: RushBSelection = RushBSelection::new(0, 0);
