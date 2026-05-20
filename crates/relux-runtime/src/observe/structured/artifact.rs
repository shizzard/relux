//! Per-test artifact listing schema and path comparator.
//!
//! An `ArtifactEntry` is a row in `StructuredLog.artifacts`. The comparator
//! orders files so that, within each directory, files come first
//! (alphabetical) before any subdirectory contents (subdirectories ordered
//! alphabetically by name, recursing).

use std::cmp::Ordering;

use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../viewer/src/types/")
)]
pub struct ArtifactEntry {
    /// Path relative to the test's artifacts directory, using forward
    /// slashes. Never starts with `/` or contains `.`/`..` segments.
    pub path: String,
    /// File size in bytes. Captured at scan time.
    pub size: u64,
    /// MIME type derived from the filename extension via `mime_guess`.
    /// `None` when no mapping exists (e.g. extensionless files, unknown
    /// extensions). The browser does its own sniffing on click.
    pub mime: Option<String>,
}

/// Order artifact paths so that within each directory level, files come
/// first (alphabetical), then subdirectory contents (subdirectories ordered
/// alphabetically by name, recursing).
pub fn cmp_artifact_paths(a: &str, b: &str) -> Ordering {
    let a_segs: Vec<&str> = a.split('/').collect();
    let b_segs: Vec<&str> = b.split('/').collect();
    let mut i = 0;
    loop {
        let a_leaf = i == a_segs.len().saturating_sub(1);
        let b_leaf = i == b_segs.len().saturating_sub(1);
        match (a_leaf, b_leaf) {
            (true, true) => return a_segs[i].cmp(b_segs[i]),
            (true, false) => return Ordering::Less,
            (false, true) => return Ordering::Greater,
            (false, false) => match a_segs[i].cmp(b_segs[i]) {
                Ordering::Equal => i += 1,
                other => return other,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;

    fn sort(mut input: Vec<&str>) -> Vec<String> {
        input.sort_by(|a, b| cmp_artifact_paths(a, b));
        input.into_iter().map(String::from).collect()
    }

    #[test]
    fn root_files_alphabetical() {
        assert_eq!(
            sort(vec!["screenshot.png", "out.txt", "audio.mp3"]),
            vec!["audio.mp3", "out.txt", "screenshot.png"],
        );
    }

    #[test]
    fn root_files_precede_subdir_contents() {
        assert_eq!(
            sort(vec!["sut/access.log", "out.txt", "screenshot.png"]),
            vec!["out.txt", "screenshot.png", "sut/access.log"],
        );
    }

    #[test]
    fn files_in_dir_precede_deeper_subdir_contents() {
        assert_eq!(
            sort(vec!["sut/logs/foo.log", "sut/error.log", "sut/access.log",]),
            vec!["sut/access.log", "sut/error.log", "sut/logs/foo.log",],
        );
    }

    #[test]
    fn worked_example_from_spec() {
        assert_eq!(
            sort(vec![
                "sut/logs/foo.log",
                "screenshot.png",
                "sut/error.log",
                "out.txt",
                "sut/access.log",
            ]),
            vec![
                "out.txt",
                "screenshot.png",
                "sut/access.log",
                "sut/error.log",
                "sut/logs/foo.log",
            ],
        );
    }

    #[test]
    fn subdirs_alphabetical_by_dir_name_then_contents() {
        assert_eq!(
            sort(vec!["b/x", "a/z", "a/a", "b/a"]),
            vec!["a/a", "a/z", "b/a", "b/x"],
        );
    }

    #[test]
    fn equal_paths_are_equal() {
        assert_eq!(cmp_artifact_paths("a/b/c", "a/b/c"), Ordering::Equal);
    }
}
