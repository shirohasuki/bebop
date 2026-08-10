use std::path::{Path, PathBuf};

use super::core_config::{parse_core_config, CoreTopology};
use super::toml_utils;

pub struct TileTopology {
    pub cores: Vec<(String, PathBuf, CoreTopology)>,
    pub virtual_bank_count: usize,
    pub files_read: Vec<PathBuf>,
}

/// A tile owns the ordered Core instance list. It may use explicit [[cores]]
/// for heterogeneous tiles or [coreTemplate] for repeated homogeneous Cores.
pub fn parse_tile_config(path: &Path) -> TileTopology {
    let mut files_read = Vec::new();
    let tile = toml_utils::read(path, &mut files_read);
    let parent = path.parent().expect("Tile TOML has parent directory");
    let mut cores = Vec::new();

    if let Some(entries) = tile.get("cores").and_then(toml::Value::as_array) {
        for entry in entries {
            add_core(entry, parent, path, &mut files_read, &mut cores);
        }
    }
    if let Some(template) = tile.get("coreTemplate") {
        let count = toml_utils::usize(template, "count", path);
        for _ in 0..count {
            add_core(template, parent, path, &mut files_read, &mut cores);
        }
    }
    assert!(!cores.is_empty(), "{} must define [[cores]] or [coreTemplate]", path.display());
    let virtual_bank_count = tile
        .get("sharedMem")
        .and_then(|value| value.get("virtualBankCount"))
        .and_then(toml::Value::as_integer)
        .map(|value| usize::try_from(value).unwrap_or_else(|_| panic!("{} sharedMem.virtualBankCount must be non-negative", path.display())))
        .unwrap_or_else(|| cores.iter().map(|(_, _, core)| core.mem_config.bank_num).max().unwrap());
    TileTopology { cores, virtual_bank_count, files_read }
}

fn add_core(
    entry: &toml::Value,
    parent: &Path,
    tile_path: &Path,
    files_read: &mut Vec<PathBuf>,
    cores: &mut Vec<(String, PathBuf, CoreTopology)>,
) {
    let core_path = toml_utils::resolve(parent, &toml_utils::string(entry, "include", tile_path));
    let core = parse_core_config(&core_path);
    files_read.extend(core.files_read.iter().cloned());
    let name = entry
        .get("name")
        .and_then(toml::Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            core_path
                .parent()
                .and_then(Path::file_name)
                .and_then(|name| name.to_str())
                .unwrap_or("core")
                .to_string()
        });
    cores.push((name, core_path, core));
}
