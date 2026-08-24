//! Corpus enumeration shared by every property test, and the losslessness
//! property itself.
//!
//! T05 extends `corpus` with `.relux` fenced blocks from `docs/**/*.md` and a
//! CRLF twin per entry, and adds the truncation property and the differential
//! oracle against chumsky.

use std::path::Path;
use std::path::PathBuf;

/// Bounds on the corpus size, asserted in both directions.
///
/// The floor catches a walker that finds nothing -- a wrong root, or a fixture
/// directory that moved. The ceiling catches something a floor cannot see: the
/// repository has 4,112 `.relux` files under `tests/` but only 255 fixtures,
/// the rest being e2e run artifacts under gitignored `out/` directories that
/// `just clean-logs` wipes. Drop the `out/` skip in `collect` and this corpus
/// becomes 266 files on clean CI and 4,123 on a machine that has run
/// `just test-e2e` -- a green suite proving something different on every
/// machine. The count is 266 today.
const CORPUS_MIN: usize = 200;
const CORPUS_MAX: usize = 400;

/// The workspace root. This crate lives at `<root>/crates/relux-parser`.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/relux-parser sits two levels below the workspace root")
        .to_path_buf()
}

/// Recursively collect `.relux` files, skipping e2e run artifacts.
fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_dir() {
            // `tests/relux/.gitignore` is exactly `out/`: these hold e2e run
            // output, including copies of the fixtures themselves.
            if path.file_name().is_some_and(|name| name == "out") {
                continue;
            }
            collect(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "relux") {
            out.push(path);
        }
    }
}

/// Every `.relux` file in the repository, as `(path, contents)`.
///
/// Read at test time rather than vendored, so a fixture added anywhere under
/// these roots joins the corpus automatically. Two explicit roots are used
/// rather than a walk from the workspace root with a skip-list, because the
/// latter needs `target`, `.direnv`, `.claude`, `.worktrees` and
/// `viewer/node_modules` exclusions maintained as the repository grows, and
/// gains a silent hole every time a build directory is added.
pub fn corpus() -> Vec<(PathBuf, String)> {
    let root = workspace_root();
    let mut paths = Vec::new();

    collect(&root.join("tests/relux"), &mut paths);
    collect(&root.join("docs"), &mut paths);

    // `read_dir` order is unspecified; sort so failures are reproducible.
    paths.sort();

    paths
        .into_iter()
        .map(|path| {
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
            (path, text)
        })
        .collect()
}

#[test]
fn corpus_is_the_tracked_fixture_set() {
    let files = corpus();
    println!("corpus: {} files", files.len());

    assert!(
        files.len() >= CORPUS_MIN,
        "corpus has {} files, expected at least {CORPUS_MIN}; the walker found \
         nothing -- check the roots in `corpus`",
        files.len()
    );
    assert!(
        files.len() <= CORPUS_MAX,
        "corpus has {} files, expected at most {CORPUS_MAX}; the `out/` skip in \
         `collect` is probably gone, pulling in e2e run artifacts",
        files.len()
    );
}
