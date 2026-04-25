fn main() {
    let version_file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../version.txt");
    let version = std::fs::read_to_string(&version_file)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", version_file.display()));
    println!("cargo:rustc-env=RELUX_VERSION={}", version.trim());
    println!("cargo:rerun-if-changed={}", version_file.display());
}
