use std::cell::RefCell;
use std::path::{Path, PathBuf};

#[allow(dead_code)]
#[path = "../../build_support/config_loader.rs"]
mod config_loader;

use config_loader::Topology;

thread_local! {
    static TOPOLOGY: RefCell<Option<Topology>> = const { RefCell::new(None) };
    static VIRTUAL_BANK_COUNT: RefCell<Option<usize>> = const { RefCell::new(None) };
}

/// Bind the current host thread to one concrete Core TOML. Every BEMU worker
/// calls this before creating its Spike/native state, so bank geometry and Ball
/// dispatch never leak between Core instances.
pub fn configure(path: &Path) {
    TOPOLOGY.with(|slot| *slot.borrow_mut() = Some(config_loader::parse_core_config(path)));
    VIRTUAL_BANK_COUNT.with(|slot| *slot.borrow_mut() = None);
}

/// Bind the current host thread from a top-level Chip TOML, following its
/// Tile and Core includes to the Core which owns the BallDomain.
pub fn configure_topology(path: &Path) {
    TOPOLOGY.with(|slot| *slot.borrow_mut() = Some(config_loader::parse_topology(path)));
    VIRTUAL_BANK_COUNT.with(|slot| *slot.borrow_mut() = None);
}

pub fn configure_with_virtual_bank_count(path: &Path, virtual_bank_count: usize) {
    configure(path);
    VIRTUAL_BANK_COUNT.with(|slot| *slot.borrow_mut() = Some(virtual_bank_count));
}

fn top_config_path() -> PathBuf {
    let path = Path::new(crate::BEMU_TOP_CONFIG);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(path)
    }
}

fn with_topology<R>(f: impl FnOnce(&Topology) -> R) -> R {
    TOPOLOGY.with(|slot| {
        if slot.borrow().is_none() {
            *slot.borrow_mut() = Some(config_loader::parse_topology(&top_config_path()));
        }
        f(slot.borrow().as_ref().expect("BEMU Core topology is not configured"))
    })
}

pub fn bank_num() -> usize {
    let private_bank_count = with_topology(|t| t.mem_config.bank_num);
    VIRTUAL_BANK_COUNT.with(|slot| slot.borrow().unwrap_or(private_bank_count).max(private_bank_count))
}
pub fn bank_width() -> usize { with_topology(|t| t.mem_config.bank_width) }
pub fn bank_lines() -> usize { with_topology(|t| t.mem_config.bank_entries) }
pub fn bank_row_bytes() -> usize { bank_width() / 8 }
pub fn bank_size() -> usize { bank_lines() * bank_row_bytes() }
pub fn mmio_enable() -> bool { with_topology(|t| t.mem_config.mmio_enable) }
pub fn mmio_bank_num() -> usize { with_topology(|t| t.mem_config.mmio_bank_num) }
pub fn mmio_bank_width() -> usize { with_topology(|t| t.mem_config.mmio_bank_width) }
pub fn mmio_bank_lines() -> usize { with_topology(|t| t.mem_config.mmio_bank_entries) }
pub fn mmio_bank_row_bytes() -> usize { mmio_bank_width() / 8 }
pub fn mmio_bank_size() -> usize { mmio_bank_lines() * mmio_bank_row_bytes() }

#[allow(dead_code)]
pub fn mmio_read_width() -> usize { with_topology(|t| t.mem_config.mmio_read_width) }
pub fn mmio_total_size() -> usize { mmio_bank_num() * mmio_bank_size() }

pub mod ball_domain {
    use super::with_topology;

    pub fn ball_class_for_funct(funct7: u32) -> Option<String> {
        with_topology(|topology| {
            let bid = topology.ball_domain.isa.iter().find(|entry| entry.funct7 == funct7)?.bid;
            topology
                .ball_domain
                .mappings
                .iter()
                .find(|mapping| mapping.ball_id == bid)
                .map(|mapping| mapping.ball_class.clone())
        })
    }
}
