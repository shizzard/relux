use std::path::Path;

use crate::diagnostics::ModulePath;

/// Discover test module paths from the given test directory.
///
/// Walks the directory recursively, finds all `.relux` files, and converts
/// their absolute paths to `ModulePath`s relative to the relux directory
/// (`{project_root}/relux/`). Results are sorted for deterministic ordering.
///
/// Library modules are NOT discovered here — they are loaded on demand
/// when imported.
pub fn discover_test_modules(test_dir: &Path, relux_dir: &Path) -> Vec<ModulePath> {
    let files = crate::dsl::discover_relux_files(test_dir);
    let mut modules: Vec<ModulePath> = files
        .into_iter()
        .filter_map(|abs_path| {
            let rel = abs_path.strip_prefix(relux_dir).ok()?;
            let without_ext = rel.with_extension("");
            let mod_path = without_ext.to_string_lossy().replace('\\', "/");
            Some(ModulePath(mod_path))
        })
        .collect();
    modules.sort_by(|a, b| a.0.cmp(&b.0));
    modules
}
