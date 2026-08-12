//! BEMU configuration entry points, organized by hardware hierarchy.

#[path = "core_config.rs"]
mod core_config;
#[path = "tile_config.rs"]
mod tile_config;
#[path = "toml_utils.rs"]
mod toml_utils;

pub use core_config::{parse_core_config, CoreTopology};
pub use tile_config::parse_tile_config;

use std::fs;
use std::path::{Path, PathBuf};

pub type Topology = CoreTopology;

pub fn top_config_from_manifest(manifest_dir: &Path) -> PathBuf {
    let lib = manifest_dir.join("src/lib.rs");
    let source = fs::read_to_string(&lib).unwrap_or_else(|error| panic!("failed to read {}: {error}", lib.display()));
    let rel = source
        .split("BEMU_TOP_CONFIG")
        .nth(1)
        .and_then(|rest| rest.split('"').nth(1))
        .unwrap_or_else(|| panic!("{} must define BEMU_TOP_CONFIG", lib.display()));
    toml_utils::resolve(lib.parent().expect("lib.rs has parent"), rel)
}

/// Follow the Chip -> Tile -> Core include chain and return the first Core
/// which owns a BallDomain. A BEMU crate is built for one chip, while workers
/// are bound to concrete Core TOMLs later at runtime.
pub fn parse_topology(chip_config: &Path) -> CoreTopology {
    let mut files_read = Vec::new();
    let chip = toml_utils::read(chip_config, &mut files_read);
    let parent = chip_config.parent().expect("Chip TOML has parent directory");

    let tile_entry = chip
        .get("tiles")
        .and_then(toml::Value::as_array)
        .and_then(|tiles| tiles.first())
        .or_else(|| chip.get("tileTemplate"))
        .unwrap_or_else(|| panic!("{} must define [[tiles]] or [tileTemplate]", chip_config.display()));
    let tile_path = toml_utils::resolve(parent, &toml_utils::string(tile_entry, "include", chip_config));
    let tile = parse_tile_config(&tile_path);
    files_read.extend(tile.files_read.iter().cloned());

    let mut core = tile
        .cores
        .into_iter()
        .map(|(_, _, core)| core)
        .find(|core| !core.ball_domain.mappings.is_empty())
        .unwrap_or_else(|| panic!("{} contains no Core with a BallDomain", tile_path.display()));
    core.files_read = files_read;
    core
}
