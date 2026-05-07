use std::path::Path;

fn main() {
    let bundle = "../../vendor/relux-viewer.js.gz";
    let bundle_path = Path::new(bundle);

    // Re-run if the bundle changes so a rebuild after `just viewer-build`
    // picks up new bytes.
    println!("cargo:rerun-if-changed={bundle}");

    if !bundle_path.exists() {
        // Hard fail with an actionable message. Without this, the user
        // gets `include_bytes!`'s opaque "couldn't read file" error
        // pointing inside src/viewer.rs, which is non-obvious.
        panic!(
            "viewer bundle not found at {bundle}\n\
             this file is committed; restore it from git, or regenerate via `just viewer-build`."
        );
    }
}
