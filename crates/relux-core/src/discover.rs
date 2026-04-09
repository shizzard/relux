use std::path::Path;
use std::path::PathBuf;

use walkdir::WalkDir;

use crate::config;

/// Recursively discover `.relux` files in a directory, stopping at nested
/// project boundaries (directories containing `Relux.toml`).
pub fn discover_relux_files(dir: &Path) -> Vec<PathBuf> {
    if !dir.is_dir() {
        return Vec::new();
    }
    let mut files: Vec<PathBuf> = WalkDir::new(dir)
        .into_iter()
        .filter_entry(|e| {
            if e.path() == dir {
                return true;
            }
            if e.file_type().is_dir() && e.path().join(config::CONFIG_FILE).exists() {
                return false;
            }
            true
        })
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "relux"))
        .map(|e| e.into_path())
        .collect();
    files.sort();
    files
}
