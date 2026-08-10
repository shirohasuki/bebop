//! BEMU configuration entry points, organized by hardware hierarchy.

#[path = "core_config.rs"]
mod core_config;
#[path = "tile_config.rs"]
mod tile_config;
#[path = "toml_utils.rs"]
mod toml_utils;

pub use core_config::{parse_core_config, CoreTopology};
#[allow(unused_imports)]
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

/// Build-time BEMU validation always targets its owning Core package.
pub fn parse_topology(core_config: &Path) -> CoreTopology {
    parse_core_config(core_config)
}
