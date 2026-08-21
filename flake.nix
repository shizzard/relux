{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { nixpkgs, rust-overlay, ... }:
    let
      systems = [ "aarch64-darwin" "x86_64-darwin" "x86_64-linux" "aarch64-linux" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f system);
    in {
      devShells = forAllSystems (system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
          };
          # Pinned, not `stable.latest`. A floating "latest" resolved to a
          # different compiler on every machine depending on when each
          # developer last updated the lock, so a clippy lint added in 1.98
          # could fail CI on a branch that passed locally. Keep this version
          # in step with `rust-toolchain.toml` and the workflow pins in
          # `.github/workflows/`.
          rust = pkgs.rust-bin.stable."1.98.0".default.override {
            extensions = [ "rust-src" "rust-analyzer" "clippy" ];
            targets = [];
          };
        in {
          default = pkgs.mkShell {
            buildInputs = [
              rust
              pkgs.just
              pkgs.mdbook
              pkgs.jdk17
              pkgs.gradle
              pkgs.jq
            ];
          };
        });
    };
}
