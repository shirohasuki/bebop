use bebop_rushb::decode_core_id;
use prost::Message;
use std::path::PathBuf;

include!(concat!(env!("OUT_DIR"), "/buckyball.config.rs"));

const CHIP_PB: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../chip.pb"));

#[derive(Clone)]
pub struct Topology {
    pub mem_config: MemConfig,
    pub ball_domain: BallDomainConfig,
}

#[derive(Clone)]
pub struct MemConfig {
    pub bank_num: usize,
    pub bank_width: usize,
    pub bank_entries: usize,
    pub mmio_enable: bool,
    pub mmio_bank_num: usize,
    pub mmio_bank_entries: usize,
    pub mmio_bank_width: usize,
    pub mmio_read_width: usize,
}

#[derive(Clone)]
pub struct BallDomainConfig {
    pub mappings: Vec<BallIdMapping>,
    pub isa: Vec<BallIsaEntry>,
}

pub struct TileTopology {
    pub cores: Vec<(String, usize)>,
    pub virtual_bank_count: usize,
}

pub struct RushBEndpoint {
    pub core_index: usize,
    pub virtual_bank_count: usize,
}

fn chip() -> Chip {
    Chip::decode(CHIP_PB).unwrap_or_else(|e| panic!("decode chip.pb: {e}"))
}

fn mem_of(core: &CoreInstance) -> &MemDomainConfig {
    core.mem
        .as_ref()
        .unwrap_or_else(|| panic!("core {} missing mem", core.index))
}

fn ball_of(core: &CoreInstance) -> &BallDomain {
    core.balldomain
        .as_ref()
        .unwrap_or_else(|| panic!("core {} missing balldomain", core.index))
}

fn to_topology(core: &CoreInstance) -> Topology {
    let mem = mem_of(core);
    let bank = mem
        .bank
        .as_ref()
        .unwrap_or_else(|| panic!("core {} missing bank", core.index));
    let mmio = mem
        .mmio
        .as_ref()
        .unwrap_or_else(|| panic!("core {} missing mmio", core.index));
    let ball = ball_of(core);
    Topology {
        mem_config: MemConfig {
            bank_num: bank.num as usize,
            bank_width: bank.width as usize,
            bank_entries: bank.entries as usize,
            mmio_enable: mmio.enable,
            mmio_bank_num: mmio.bank_num as usize,
            mmio_bank_entries: mmio.bank_entries as usize,
            mmio_bank_width: mmio.bank_width as usize,
            mmio_read_width: mmio.read_width as usize,
        },
        ball_domain: BallDomainConfig {
            mappings: ball.mappings.iter().cloned().collect(),
            isa: ball.isa.iter().cloned().collect(),
        },
    }
}

pub fn default_core() -> Topology {
    let c = chip();
    let core = c.cores.first().unwrap_or_else(|| panic!("chip.pb has no cores"));
    to_topology(core)
}

pub fn topology_for_core(core_index: usize) -> Topology {
    let c = chip();
    let core = c.cores.get(core_index).unwrap_or_else(|| {
        panic!(
            "core index {core_index} out of range (n={})",
            c.cores.len()
        )
    });
    to_topology(core)
}

pub fn rushb_endpoint(core_id: u32) -> RushBEndpoint {
    let (tile_id, local_id) = decode_core_id(core_id);
    let c = chip();
    let tile = c.tiles.get(tile_id as usize).unwrap_or_else(|| {
        panic!(
            "rushB Core {core_id}: tile {tile_id} out of range (n={})",
            c.tiles.len()
        )
    });
    let core_index = *tile.core_indices.get(local_id as usize).unwrap_or_else(|| {
        panic!(
            "rushB Core {core_id}: local index {local_id} out of range for tile {tile_id} (n={})",
            tile.core_indices.len()
        )
    }) as usize;
    let core = c.cores.get(core_index).unwrap_or_else(|| {
        panic!(
            "rushB Core {core_id}: config index {core_index} out of range (n={})",
            c.cores.len()
        )
    });
    if core
        .balldomain
        .as_ref()
        .map_or(true, |ball| ball.mappings.is_empty())
    {
        panic!("rushB Core {core_id}: config index {core_index} has no Buckyball mappings");
    }
    if tile.virtual_bank_count == 0 {
        panic!("rushB Core {core_id}: tile {tile_id} virtual_bank_count is 0");
    }
    RushBEndpoint {
        core_index,
        virtual_bank_count: tile.virtual_bank_count as usize,
    }
}

pub fn tile_topology(tile_index: usize) -> TileTopology {
    let c = chip();
    let tile = c.tiles.get(tile_index).unwrap_or_else(|| {
        panic!(
            "tile index {tile_index} out of range (n={})",
            c.tiles.len()
        )
    });
    if tile.core_indices.is_empty() {
        panic!("tile {tile_index} has no cores");
    }
    if tile.virtual_bank_count == 0 {
        panic!("tile {tile_index} virtual_bank_count is 0");
    }
    let cores = tile
        .core_indices
        .iter()
        .map(|&index| {
            let index = index as usize;
            let role = c.cores[index].role.clone();
            (role, index)
        })
        .collect();
    TileTopology {
        cores,
        virtual_bank_count: tile.virtual_bank_count as usize,
    }
}

pub fn chip_manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
