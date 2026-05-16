# Default: show available targets
default:
    @just --list

# Build in debug mode
build:
    cargo build

# Regenerate the vendored Svelte viewer bundle (vendor/relux-viewer.js.gz).
# Drives ts-rs schema export → docker npm build → copy to vendor/. The
# pre-commit hook (.githooks/pre-commit) and CI verify the vendored bytes
# stay in sync with viewer/ sources.
viewer-build:
    cargo test -p relux-runtime --features ts-export 'export_bindings_'
    docker run --rm -v {{justfile_directory()}}/viewer:/src -w /src node:lts-slim \
        sh -c 'npm ci && npm run build'
    cp viewer/dist/relux-viewer.js.gz vendor/relux-viewer.js.gz

# Run viewer unit tests (vitest).
viewer-test:
    docker run --rm -v {{justfile_directory()}}/viewer:/src -w /src node:lts-slim \
        sh -c 'npm ci && npm test'

# Type/Svelte-check the viewer (svelte-check).
viewer-check:
    docker run --rm -v {{justfile_directory()}}/viewer:/src -w /src node:lts-slim \
        sh -c 'npm ci && npm run check'

# Configure git to use the repo-local hooks directory (.githooks/).
install-hooks:
    git config core.hooksPath .githooks

# Build in release mode
release:
    cargo build --release

# Run all checks: cargo check + clippy + fmt
check: check-clippy check-fmt

# Run clippy check (includes cargo check)
check-clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# Run formatter check
check-fmt:
    rustup run nightly rustfmt --edition 2024 --check $(find crates -name '*.rs')

# Fix clippy warnings and format code
fix:
    cargo clippy --workspace --all-targets --fix --allow-dirty --allow-staged -- -D warnings
    rustup run nightly rustfmt --edition 2024 $(find crates -name '*.rs')

# Build tutorial books
books: _sync-book-assets
    mdbook build docs/dsl-tutorial
    mdbook build docs/suite-tutorial
    mdbook build docs/reference

# Copy the canonical hljs grammar, theme CSS, and the vendored
# highlight.js v11 into each book directory. The vendored hljs goes
# into `theme/highlight.js` — mdBook's theme-override mechanism
# replaces its built-in v10.1.1 with our v11 so the runtime report and
# the books use the same hljs version (and grammar). The per-book
# copies are all gitignored; this step regenerates them before mdbook
# reads them.
_sync-book-assets:
    #!/usr/bin/env bash
    set -euo pipefail
    hljs_gz=vendor/highlight-11.11.1.min.js.gz
    grammar=crates/relux-runtime/src/report/highlight-relux.js
    css=docs/_theme/relux.css
    for book in docs/dsl-tutorial docs/reference docs/suite-tutorial; do
        mkdir -p "$book/theme"
        gunzip -c "$hljs_gz" > "$book/theme/highlight.js"
        cp "$grammar" "$book/highlight-relux.js"
        cp "$css" "$book/relux.css"
    done

# Run all tests (unit + e2e)
test: unit e2e

# Run unit tests
unit *ARGS:
    cargo test --workspace --lib {{ARGS}}

# Run relux with arguments
run *ARGS:
    cargo run -p relux -- {{ARGS}}

# Run e2e tests (check then run)
e2e:
    cargo run -p relux -- check --manifest tests/Relux.toml
    cargo run -p relux -- run --manifest tests/Relux.toml

# Analyze run history
history *ARGS:
    cargo run -p relux -- history --manifest tests/Relux.toml {{ARGS}}

# Build the IntelliJ plugin
intellij:
    cd editors/intellij && gradle buildPlugin --info

# Build the VS Code extension (.vsix)
vscode:
    mkdir -p editors/vscode/build
    docker run --rm -v {{justfile_directory()}}/editors/vscode:/src -w /src node:lts-slim \
        sh -c 'npx --yes @vscode/vsce package --out /src/build/relux.vsix'

# Remove build artifacts
clean:
    cargo clean

# Remove e2e test output logs
clean-logs:
    rm -rf tests/relux/out/run-*
    rm -f tests/relux/out/latest
