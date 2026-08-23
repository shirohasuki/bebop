use std::path::PathBuf;

use serde::Deserialize;

include!(concat!(env!("OUT_DIR"), "/bundle_embed.rs"));

#[derive(Clone, Deserialize)]
pub struct Topology {
    pub mem_config: MemConfig,
    pub ball_domain: BallDomainConfig,
}

#[derive(Clone, Deserialize)]
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

#[derive(Clone, Deserialize)]
pub struct BallDomainConfig {
    pub mappings: Vec<BallIdMapping>,
    pub isa: Vec<BallIsaEntry>,
}

#[derive(Clone, Deserialize)]
pub struct BallIdMapping {
    #[serde(rename = "ballId", default)]
    pub ball_id: u32,
    #[serde(rename = "ballClass")]
    pub ball_class: String,
}

#[derive(Clone, Deserialize)]
pub struct BallIsaEntry {
    pub mnemonic: String,
    pub funct7: u32,
    #[serde(default)]
    pub bid: u32,
}

pub struct TileTopology {
    pub cores: Vec<(String, usize)>,
    pub virtual_bank_count: usize,
}

#[derive(Deserialize)]
struct ChipBundle {
    tiles: Vec<TilePlacement>,
    cores: Vec<CoreInstance>,
}

#[derive(Deserialize)]
struct TilePlacement {
    #[serde(rename = "virtualBankCount", default)]
    virtual_bank_count: u32,
    #[serde(rename = "coreIndices")]
    core_indices: Vec<u32>,
}

#[derive(Deserialize)]
struct CoreInstance {
    role: String,
    balldomain: BallDomainRaw,
    mem: MemRaw,
}

#[derive(Deserialize)]
struct BallDomainRaw {
    mappings: Vec<BallIdMapping>,
    isa: Vec<BallIsaEntry>,
}

#[derive(Deserialize)]
struct MemRaw {
    bank: BankRaw,
    mmio: MmioRaw,
}

#[derive(Deserialize)]
struct BankRaw {
    num: u32,
    width: u32,
    entries: u32,
}

#[derive(Deserialize)]
struct MmioRaw {
    #[serde(default)]
    enable: bool,
    #[serde(rename = "bankNum")]
    bank_num: u32,
    #[serde(rename = "bankEntries")]
    bank_entries: u32,
    #[serde(rename = "bankWidth")]
    bank_width: u32,
    #[serde(rename = "readWidth")]
    read_width: u32,
}

fn bundle() -> ChipBundle {
    serde_json::from_str(CHIP_BUNDLE_JSON).expect("invalid chip bundle JSON")
}

fn to_topology(core: &CoreInstance) -> Topology {
    Topology {
        mem_config: MemConfig {
            bank_num: core.mem.bank.num as usize,
            bank_width: core.mem.bank.width as usize,
            bank_entries: core.mem.bank.entries as usize,
            mmio_enable: core.mem.mmio.enable,
            mmio_bank_num: core.mem.mmio.bank_num as usize,
            mmio_bank_entries: core.mem.mmio.bank_entries as usize,
            mmio_bank_width: core.mem.mmio.bank_width as usize,
            mmio_read_width: core.mem.mmio.read_width as usize,
        },
        ball_domain: BallDomainConfig {
            mappings: core.balldomain.mappings.clone(),
            isa: core.balldomain.isa.clone(),
        },
    }
}

pub fn default_core() -> Topology {
    let cores = bundle().cores;
    let core = cores
        .iter()
        .find(|core| !core.balldomain.mappings.is_empty())
        .unwrap_or_else(|| cores.first().expect("chip bundle has no cores"));
    to_topology(core)
}

pub fn topology_for_core(core_index: usize) -> Topology {
    let cores = bundle().cores;
    let core = cores
        .get(core_index)
        .unwrap_or_else(|| panic!("core index {core_index} out of range (n={})", cores.len()));
    to_topology(core)
}

pub fn tile_topology(tile_index: usize) -> TileTopology {
    let bundle = bundle();
    let tile = bundle.tiles.get(tile_index).unwrap_or_else(|| {
        panic!(
            "tile index {tile_index} out of range (n={})",
            bundle.tiles.len()
        )
    });
    if tile.core_indices.is_empty() {
        panic!("tile {tile_index} has no cores");
    }
    let virtual_bank_count = if tile.virtual_bank_count > 0 {
        tile.virtual_bank_count as usize
    } else {
        tile.core_indices
            .iter()
            .map(|&index| bundle.cores[index as usize].mem.bank.num as usize)
            .max()
            .expect("tile has no cores")
    };
    let cores = tile
        .core_indices
        .iter()
        .map(|&index| {
            let index = index as usize;
            let role = bundle.cores[index].role.clone();
            (role, index)
        })
        .collect();
    TileTopology {
        cores,
        virtual_bank_count,
    }
}

pub fn chip_manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
