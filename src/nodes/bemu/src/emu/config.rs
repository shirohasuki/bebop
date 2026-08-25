use std::cell::RefCell;

mod chip_config;

pub use chip_config::{tile_topology, TileTopology, Topology};

thread_local! {
    static TOPOLOGY: RefCell<Option<Topology>> = const { RefCell::new(None) };
    static VIRTUAL_BANK_COUNT: RefCell<Option<usize>> = const { RefCell::new(None) };
}

pub fn configure_core(core_index: usize) {
    TOPOLOGY.with(|slot| *slot.borrow_mut() = Some(chip_config::topology_for_core(core_index)));
    VIRTUAL_BANK_COUNT.with(|slot| *slot.borrow_mut() = None);
}

pub fn configure_default() {
    TOPOLOGY.with(|slot| *slot.borrow_mut() = Some(chip_config::default_core()));
    VIRTUAL_BANK_COUNT.with(|slot| *slot.borrow_mut() = None);
}

pub fn configure_core_with_virtual_bank_count(core_index: usize, virtual_bank_count: usize) {
    configure_core(core_index);
    VIRTUAL_BANK_COUNT.with(|slot| *slot.borrow_mut() = Some(virtual_bank_count));
}

fn with_topology<R>(f: impl FnOnce(&Topology) -> R) -> R {
    TOPOLOGY.with(|slot| {
        let borrow = slot.borrow();
        let topology = borrow.as_ref().unwrap_or_else(|| panic!("BEMU topology is not configured"));
        f(topology)
    })
}

pub fn bank_num() -> usize {
    let private_bank_count = with_topology(|t| t.mem_config.bank_num);
    VIRTUAL_BANK_COUNT.with(|slot| slot.borrow().unwrap_or(private_bank_count).max(private_bank_count))
}
pub fn bank_width() -> usize {
    with_topology(|t| t.mem_config.bank_width)
}
pub fn bank_lines() -> usize {
    with_topology(|t| t.mem_config.bank_entries)
}
pub fn bank_row_bytes() -> usize {
    bank_width() / 8
}
pub fn bank_size() -> usize {
    bank_lines() * bank_row_bytes()
}
pub fn mmio_enable() -> bool {
    with_topology(|t| t.mem_config.mmio_enable)
}
pub fn mmio_bank_num() -> usize {
    with_topology(|t| t.mem_config.mmio_bank_num)
}
pub fn mmio_bank_width() -> usize {
    with_topology(|t| t.mem_config.mmio_bank_width)
}
pub fn mmio_bank_lines() -> usize {
    with_topology(|t| t.mem_config.mmio_bank_entries)
}
pub fn mmio_bank_row_bytes() -> usize {
    mmio_bank_width() / 8
}
pub fn mmio_bank_size() -> usize {
    mmio_bank_lines() * mmio_bank_row_bytes()
}

#[allow(dead_code)]
pub fn mmio_read_width() -> usize {
    with_topology(|t| t.mem_config.mmio_read_width)
}
pub fn mmio_total_size() -> usize {
    mmio_bank_num() * mmio_bank_size()
}

pub mod ball_domain {
    use super::with_topology;

    pub fn ball_class_for_funct(funct7: u32) -> Option<String> {
        with_topology(|topology| {
            let bid = topology
                .ball_domain
                .isa
                .iter()
                .find(|entry| entry.funct7 == funct7)?
                .bid;
            topology
                .ball_domain
                .mappings
                .iter()
                .find(|mapping| mapping.ball_id == bid)
                .map(|mapping| mapping.ball_class.clone())
        })
    }

    pub fn mnemonic_for_funct(funct7: u32) -> Option<String> {
        with_topology(|topology| {
            topology
                .ball_domain
                .isa
                .iter()
                .find(|entry| entry.funct7 == funct7)
                .map(|entry| entry.mnemonic.clone())
        })
    }

    pub fn funct_for_mnemonic(mnemonic: &str) -> Option<u32> {
        with_topology(|topology| {
            topology
                .ball_domain
                .isa
                .iter()
                .find(|entry| entry.mnemonic == mnemonic)
                .map(|entry| entry.funct7)
        })
    }
}
