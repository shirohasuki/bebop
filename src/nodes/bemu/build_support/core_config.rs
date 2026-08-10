use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::toml_utils;

#[derive(Clone, Copy)]
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

impl Default for MemConfig {
    fn default() -> Self {
        Self {
            bank_num: 32,
            bank_width: 128,
            bank_entries: 1024,
            mmio_enable: true,
            mmio_bank_num: 16,
            mmio_bank_entries: 64,
            mmio_bank_width: 128,
            mmio_read_width: 8,
        }
    }
}

pub struct CoreTopology {
    pub mem_config: MemConfig,
    pub ball_domain: BallDomainConfig,
    pub files_read: Vec<PathBuf>,
}

#[derive(Clone)]
pub struct BallDomainConfig {
    pub mappings: Vec<BallIdMapping>,
    pub isa: Vec<BallIsaEntry>,
}

impl BallDomainConfig {
    pub fn empty() -> Self {
        Self { mappings: Vec::new(), isa: Vec::new() }
    }
}

#[derive(Clone)]
pub struct BallIdMapping {
    pub ball_id: u32,
    pub ball_class: String,
}

#[derive(Clone)]
pub struct BallIsaEntry {
    pub funct7: u32,
    pub bid: u32,
}

pub fn parse_core_config(path: &Path) -> CoreTopology {
    let mut files_read = Vec::new();
    let core = toml_utils::read(path, &mut files_read);
    let parent = path.parent().expect("Core TOML has parent directory");
    let mem_config = core.get("memdomain").map(|value| {
        let mem_path = toml_utils::resolve(parent, value.as_str().expect("memdomain must be a string"));
        let mem = toml_utils::read(&mem_path, &mut files_read);
        parse_memory(&mem, &mem_path)
    }).unwrap_or_default();
    let ball_domain = core.get("balldomain").map(|value| {
        let ball_path = toml_utils::resolve(parent, value.as_str().expect("balldomain must be a string"));
        let balls = toml_utils::read(&ball_path, &mut files_read);
        parse_balls(&balls, &ball_path)
    }).unwrap_or_else(BallDomainConfig::empty);
    CoreTopology {
        mem_config,
        ball_domain,
        files_read,
    }
}

fn parse_memory(value: &toml::Value, path: &Path) -> MemConfig {
    let bank = toml_utils::table(value, "bank", path);
    let mmio = toml_utils::table(value, "mmio", path);
    let bank_value = toml::Value::Table(bank.clone());
    let mmio_value = toml::Value::Table(mmio.clone());
    MemConfig {
        bank_num: toml_utils::usize(&bank_value, "num", path),
        bank_width: toml_utils::usize(&bank_value, "width", path),
        bank_entries: toml_utils::usize(&bank_value, "entries", path),
        mmio_enable: toml_utils::boolean(&mmio_value, "enable", path),
        mmio_bank_num: toml_utils::usize(&mmio_value, "bankNum", path),
        mmio_bank_entries: toml_utils::usize(&mmio_value, "bankEntries", path),
        mmio_bank_width: toml_utils::usize(&mmio_value, "bankWidth", path),
        mmio_read_width: toml_utils::usize(&mmio_value, "readWidth", path),
    }
}

fn parse_balls(value: &toml::Value, path: &Path) -> BallDomainConfig {
    let mappings = value
        .get("ballIdMappings")
        .and_then(toml::Value::as_array)
        .unwrap_or_else(|| panic!("{} must define ballIdMappings", path.display()))
        .iter()
        .map(|entry| BallIdMapping {
            ball_id: integer(entry, "ballId", path),
            ball_class: toml_utils::string(entry, "ballClass", path),
        })
        .collect::<Vec<_>>();
    let isa = value
        .get("ballISA")
        .and_then(toml::Value::as_array)
        .unwrap_or_else(|| panic!("{} must define ballISA", path.display()))
        .iter()
        .map(|entry| BallIsaEntry {
            funct7: integer(entry, "funct7", path),
            bid: integer(entry, "bid", path),
        })
        .collect::<Vec<_>>();
    validate_balls(&mappings, &isa, path);
    BallDomainConfig { mappings, isa }
}

fn integer(value: &toml::Value, key: &str, path: &Path) -> u32 {
    let value = value
        .get(key)
        .and_then(toml::Value::as_integer)
        .unwrap_or_else(|| panic!("{} must define integer {key}", path.display()));
    u32::try_from(value).unwrap_or_else(|_| panic!("{} key {key} must be non-negative", path.display()))
}

fn validate_balls(mappings: &[BallIdMapping], isa: &[BallIsaEntry], path: &Path) {
    let ids: BTreeSet<_> = mappings.iter().map(|mapping| mapping.ball_id).collect();
    assert_eq!(ids.len(), mappings.len(), "{} has duplicate ballId", path.display());
    let functs: BTreeSet<_> = isa.iter().map(|entry| entry.funct7).collect();
    assert_eq!(functs.len(), isa.len(), "{} has duplicate ballISA funct7", path.display());
    for entry in isa {
        assert!(ids.contains(&entry.bid), "{} ballISA funct7 {} references missing bid {}", path.display(), entry.funct7, entry.bid);
    }
}
