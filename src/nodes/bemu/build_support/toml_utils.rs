use std::fs;
use std::path::{Path, PathBuf};

pub fn read(path: &Path, files_read: &mut Vec<PathBuf>) -> toml::Value {
    files_read.push(path.to_path_buf());
    let source = fs::read_to_string(path).unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    source
        .parse()
        .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()))
}

pub fn resolve(base: &Path, value: &str) -> PathBuf {
    let path = Path::new(value);
    if path.is_absolute() { path.to_path_buf() } else { base.join(path) }
}

pub fn string(value: &toml::Value, key: &str, path: &Path) -> String {
    value
        .get(key)
        .and_then(toml::Value::as_str)
        .unwrap_or_else(|| panic!("{} must define string key {key}", path.display()))
        .to_string()
}

pub fn usize(value: &toml::Value, key: &str, path: &Path) -> usize {
    let value = value
        .get(key)
        .and_then(toml::Value::as_integer)
        .unwrap_or_else(|| panic!("{} must define integer key {key}", path.display()));
    usize::try_from(value).unwrap_or_else(|_| panic!("{} key {key} must be non-negative", path.display()))
}

pub fn boolean(value: &toml::Value, key: &str, path: &Path) -> bool {
    value
        .get(key)
        .and_then(toml::Value::as_bool)
        .unwrap_or_else(|| panic!("{} must define boolean key {key}", path.display()))
}

pub fn table<'a>(value: &'a toml::Value, key: &str, path: &Path) -> &'a toml::map::Map<String, toml::Value> {
    value
        .get(key)
        .and_then(toml::Value::as_table)
        .unwrap_or_else(|| panic!("{} must define [{key}]", path.display()))
}
