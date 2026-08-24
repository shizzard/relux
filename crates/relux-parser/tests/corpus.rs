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

/// The size ceiling only notices a missing `out/` skip on a machine that has
/// actually run `just test-e2e`; on a clean checkout there are no artifacts to
/// pull in, so deleting the skip leaves every test green. Assert the skip
/// directly against a synthetic tree, so its removal is caught everywhere.
#[test]
fn collect_skips_e2e_run_artifacts() {
    // `CARGO_TARGET_TMPDIR` is cargo's scratch directory for integration tests,
    // so this never writes into the repository the walker is pointed at.
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("collect_skips_e2e_run_artifacts");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("out").join("run-1")).expect("creating the fixture tree");
    std::fs::write(root.join("kept.relux"), "").expect("writing kept.relux");
    std::fs::write(root.join("ignored.md"), "").expect("writing ignored.md");
    std::fs::write(root.join("out").join("run-1").join("artifact.relux"), "")
        .expect("writing artifact.relux");

    let mut found = Vec::new();
    collect(&root, &mut found);

    assert_eq!(
        found,
        vec![root.join("kept.relux")],
        "`collect` must skip `out/` and take only `.relux`: an e2e run copies \
         the fixtures into `out/`, so walking it makes the corpus depend on \
         whether this machine has run `just test-e2e`"
    );

    std::fs::remove_dir_all(&root).expect("cleaning up the fixture tree");
}

/// Parse `source` with the stub grammar and assert the tree reproduces it byte
/// for byte.
///
/// Losslessness is a property of the raw file bytes. T01 deleted
/// `relux_lexer::normalize()` and made `\r\n` a single two-byte `Newline`, so
/// offsets agree with the file on disk and with a client's editor buffer.
/// Never route a fixture through anything that rewrites line endings.
fn assert_lossless(source: &str, label: &str) {
    let mut p = relux_parser::parser::Parser::new(source);
    relux_parser::grammar::module(&mut p);
    let (tokens, events) = p.finish();

    let green = relux_parser::builder::build_tree(source, &tokens, &events);

    assert_eq!(
        green.to_string(),
        source,
        "tree is not lossless for {label}"
    );
}

#[test]
fn lossless_over_the_corpus() {
    for (path, source) in corpus() {
        assert_lossless(&source, &path.display().to_string());
    }
}

#[test]
fn lossless_over_edge_cases() {
    let cases = [
        ("empty", ""),
        ("spaces only", "   "),
        ("tabs and newlines", "\t\n\t\n"),
        ("no trailing newline", "test \"a\""),
        ("crlf", "test\r\n  send \"x\"\r\n"),
        ("lone cr", "a\rb"),
        ("single unmatched byte", "$"),
        ("garbage", "}{)(><!?~@#][,/-.:"),
        // Leaf text is `&source[start..end]`, which panics outright if a token
        // span lands off a char boundary. Nothing else in this set would catch
        // that. Written with escapes because sources are ASCII-only.
        (
            "multi byte utf8",
            "send \"caf\u{00e9} \u{4e2d}\u{6587} \u{1f600}\"",
        ),
    ];

    for (name, source) in cases {
        assert_lossless(source, name);
    }
}
