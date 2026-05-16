//! Filesystem walk that enumerates a test's artifact directory.
//!
//! `scan_artifacts` is called once per test, after the test body and
//! effect cleanup have run, to produce the `Vec<ArtifactEntry>` stored on
//! `StructuredLog.artifacts`. It is a best-effort capture: per-entry I/O
//! errors and symlinks are silently skipped.

use std::path::Path;

use walkdir::WalkDir;

use crate::observe::structured::ArtifactEntry;
use crate::observe::structured::artifact::cmp_artifact_paths;

/// Walk `artifacts_dir` recursively and return one `ArtifactEntry` per
/// regular file, sorted with `cmp_artifact_paths`. Directories are not
/// rows. Symlinks are skipped. A missing or unreadable directory yields
/// an empty vec.
pub fn scan_artifacts(artifacts_dir: &Path) -> Vec<ArtifactEntry> {
    let mut out = Vec::new();
    for entry in WalkDir::new(artifacts_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        let file_type = entry.file_type();
        if file_type.is_symlink() || !file_type.is_file() {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let Ok(rel) = entry.path().strip_prefix(artifacts_dir) else {
            continue;
        };
        let path = rel
            .components()
            .filter_map(|c| match c {
                std::path::Component::Normal(s) => s.to_str(),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("/");
        let mime = mime_guess::from_path(rel)
            .first()
            .map(|m| m.essence_str().to_string());
        out.push(ArtifactEntry {
            path,
            size: metadata.len(),
            mime,
        });
    }
    out.sort_by(|a, b| cmp_artifact_paths(&a.path, &b.path));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn tempdir() -> PathBuf {
        use std::sync::atomic::AtomicU64;
        use std::sync::atomic::Ordering;
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let base = std::env::temp_dir();
        let unique = format!(
            "relux-scan-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
        );
        let dir = base.join(unique);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn missing_dir_returns_empty() {
        let dir = tempdir();
        let missing = dir.join("does-not-exist");
        assert!(scan_artifacts(&missing).is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn empty_dir_returns_empty() {
        let dir = tempdir();
        assert!(scan_artifacts(&dir).is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn root_files_and_subdir_ordered_with_metadata() {
        let dir = tempdir();
        fs::write(dir.join("out.txt"), b"hello").unwrap();
        fs::write(dir.join("screenshot.png"), b"\x89PNG").unwrap();
        fs::create_dir_all(dir.join("sut/logs")).unwrap();
        fs::write(dir.join("sut/access.log"), b"AAA").unwrap();
        fs::write(dir.join("sut/error.log"), b"BB").unwrap();
        fs::write(dir.join("sut/logs/foo.log"), b"C").unwrap();
        let entries = scan_artifacts(&dir);
        assert_eq!(
            entries.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(),
            vec![
                "out.txt",
                "screenshot.png",
                "sut/access.log",
                "sut/error.log",
                "sut/logs/foo.log",
            ],
        );
        let by_path: std::collections::HashMap<_, _> =
            entries.iter().map(|e| (e.path.as_str(), e)).collect();
        assert_eq!(by_path["out.txt"].size, 5);
        assert_eq!(by_path["sut/logs/foo.log"].size, 1);
        assert_eq!(by_path["out.txt"].mime.as_deref(), Some("text/plain"));
        assert_eq!(by_path["screenshot.png"].mime.as_deref(), Some("image/png"));
        fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_skipped() {
        let dir = tempdir();
        fs::write(dir.join("real.txt"), b"real").unwrap();
        std::os::unix::fs::symlink(dir.join("real.txt"), dir.join("link.txt")).unwrap();
        let entries = scan_artifacts(&dir);
        assert_eq!(
            entries.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(),
            vec!["real.txt"],
            "symlinks must not appear in the output",
        );
        fs::remove_dir_all(&dir).ok();
    }
}
