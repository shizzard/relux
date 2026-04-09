use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use relux_core::config;
use relux_core::discover::discover_relux_files;
use relux_runtime::report::run_summary::RunSummary;
use relux_runtime::report::run_summary::read_run_summary;

pub struct LoadedRun {
    pub dir: PathBuf,
    pub summary: RunSummary,
}

pub fn load_all_summaries(out_root: &Path, last_n: Option<usize>) -> Vec<LoadedRun> {
    let mut run_dirs: Vec<PathBuf> = match fs::read_dir(out_root) {
        Ok(entries) => entries
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with("run-") && entry.file_type().ok()?.is_dir() {
                    Some(entry.path())
                } else {
                    None
                }
            })
            .collect(),
        Err(_) => return Vec::new(),
    };

    run_dirs.sort();

    if let Some(n) = last_n {
        let skip = run_dirs.len().saturating_sub(n);
        run_dirs = run_dirs.into_iter().skip(skip).collect();
    }

    run_dirs
        .into_iter()
        .filter_map(|dir| {
            let summary = read_run_summary(&dir).ok()?;
            Some(LoadedRun { dir, summary })
        })
        .collect()
}

pub fn resolve_test_filters(project_root: &Path, raw_paths: &[PathBuf]) -> Vec<String> {
    let tests_dir = config::tests_dir(project_root);
    let mut test_paths = Vec::new();

    for path in raw_paths {
        if path.is_dir() {
            match path.canonicalize() {
                Ok(canonical) => {
                    for file in discover_relux_files(&canonical) {
                        if let Ok(rel) = file.strip_prefix(&tests_dir) {
                            test_paths.push(rel.display().to_string());
                        }
                    }
                }
                Err(e) => {
                    eprintln!("warning: cannot resolve {}: {e}", path.display());
                }
            }
        } else if path.exists() {
            match path.canonicalize() {
                Ok(canonical) => {
                    if let Ok(rel) = canonical.strip_prefix(&tests_dir) {
                        test_paths.push(rel.display().to_string());
                    }
                }
                Err(e) => {
                    eprintln!("warning: cannot resolve {}: {e}", path.display());
                }
            }
        } else {
            eprintln!("warning: path does not exist: {}", path.display());
        }
    }

    test_paths.sort();
    test_paths.dedup();
    test_paths
}

pub fn filter_summaries(runs: &mut [LoadedRun], test_paths: &[String]) {
    if test_paths.is_empty() {
        return;
    }
    let filter_set: HashSet<&str> = test_paths.iter().map(|s| s.as_str()).collect();
    for run in runs.iter_mut() {
        run.summary
            .tests
            .retain(|t| filter_set.contains(t.path.as_str()));
    }
}
