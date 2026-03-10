use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::config;

pub mod lexer;
pub mod parser;
pub mod report;
pub mod resolver;

pub use lexer::lex;
pub use parser::parse;
pub use report::{print_diagnostics, print_failure};
pub use resolver::{resolve, resolve_with};

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
