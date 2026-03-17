use std::path::{Path, PathBuf};

use crate::dsl::discover_relux_files;

use super::DiagnosticError;

pub(super) fn resolve_paths(
    paths: &[PathBuf],
    project_root: &Path,
) -> (Vec<PathBuf>, Vec<DiagnosticError>) {
    let mut files = Vec::new();
    let mut diagnostics = Vec::new();
    for path in paths {
        if path.is_dir() {
            match path.canonicalize() {
                Ok(canonical) if canonical.starts_with(project_root) => {
                    files.extend(discover_relux_files(&canonical));
                }
                Ok(_) => {
                    diagnostics.push(DiagnosticError::RootNotFound {
                        path: path.display().to_string(),
                    });
                }
                Err(_) => {
                    diagnostics.push(DiagnosticError::RootNotFound {
                        path: path.display().to_string(),
                    });
                }
            }
        } else if path.exists() {
            match path.canonicalize() {
                Ok(canonical) if canonical.starts_with(project_root) => {
                    files.push(canonical);
                }
                Ok(_) => {
                    diagnostics.push(DiagnosticError::RootNotFound {
                        path: path.display().to_string(),
                    });
                }
                Err(_) => {
                    diagnostics.push(DiagnosticError::RootNotFound {
                        path: path.display().to_string(),
                    });
                }
            }
        } else {
            diagnostics.push(DiagnosticError::RootNotFound {
                path: path.display().to_string(),
            });
        }
    }
    files.sort();
    files.dedup();
    (files, diagnostics)
}

pub(super) fn path_to_mod(path: &Path, project_root: &Path) -> String {
    path.strip_prefix(project_root)
        .unwrap_or(path)
        .with_extension("")
        .to_string_lossy()
        .replace('\\', "/")
}
