pub mod bank_matrix;
pub mod base;
pub mod decode;
#[path = "00_fence.rs"]
pub mod f00_fence;
#[path = "01_barrier.rs"]
pub mod f01_barrier;
#[path = "16_mvout.rs"]
pub mod f16_mvout;
#[path = "32_mset.rs"]
pub mod f32_mset;
#[path = "33_mvin.rs"]
pub mod f33_mvin;
#[path = "35_mvin_mmio.rs"]
pub mod f35_mvin_mmio;
pub mod instruction;
pub use base::{cycles_after_issue, execute_known};
