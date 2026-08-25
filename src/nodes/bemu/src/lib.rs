mod chip;
mod sim;

#[path = "../native/ffi.rs"]
mod ffi;

#[path = "../native/spike.rs"]
mod spike;

#[path = "emu/bank/mod.rs"]
mod bank;

#[path = "emu/config.rs"]
mod config;

#[path = "emu/inst/mod.rs"]
mod inst;

mod trace;

pub use bebop_bemu_profile::{format_report as format_profile_report, print_report as print_profile_report};
pub use config::{tile_topology, TileTopology};
pub use ffi::SharedMemory;
pub use sim::BemuInstance;
pub use trace::TraceConfig;

/// Private-bank geometry used by an in-process RTL DiffTest monitor.
/// Geometry follows chip.pb baked at build time.
pub fn private_bank_geometry() -> (usize, usize) {
    (config::bank_size(), config::bank_row_bytes())
}
