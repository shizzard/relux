# Default: show available targets
default:
    @just --list
        
# Configure git to use the repo-local hooks directory (.githooks/).
install-hooks:
    git config core.hooksPath .githooks

# Reinstall the agents/ plugin into Claude Code's plugin cache. Needed when
# iterating on agents/ in place: `claude plugin update` short-circuits when
# the plugin version matches the cached copy, so pushing edits without a
# version bump requires uninstall + install. MARKETPLACE defaults to
# "relux" (the name in .claude-plugin/marketplace.json, as registered by
# `claude plugin marketplace add .`); override to match a differently-named
# local marketplace.
install-agents MARKETPLACE="relux":
    -claude plugin uninstall relux@{{MARKETPLACE}}
    claude plugin install relux@{{MARKETPLACE}}

# Fix clippy warnings and format code
fix:
    cargo clippy --workspace --all-targets --fix --allow-dirty --allow-staged -- -D warnings
    rustup run nightly rustfmt --edition 2024 $(find crates -name '*.rs')

# Run relux with arguments
run *ARGS:
    cargo run -p relux -- {{ARGS}}

# Analyze run history
history *ARGS:
    cargo run -p relux -- history --manifest tests/Relux.toml {{ARGS}}

## Build targets

# Build in debug mode
build: build-cargo build-viewer build-intellij build-vscode build-agents build-books

build-cargo:
    cargo build

# Regenerate the vendored Svelte viewer bundle (vendor/relux-viewer.js.gz).
# Drives ts-rs schema export -> docker npm build -> copy to vendor/. The
# pre-commit hook (.githooks/pre-commit) and CI verify the vendored bytes
# stay in sync with viewer/ sources.
build-viewer:
    cargo test -p relux-runtime --features ts-export 'export_bindings_'
    docker run --rm -v {{justfile_directory()}}/viewer:/src -w /src node:lts-slim \
        sh -c 'npm ci && npm run build'
    cp viewer/dist/relux-viewer.js.gz vendor/relux-viewer.js.gz

# Build the IntelliJ plugin
build-intellij:
    cd editors/intellij && gradle buildPlugin --info

# Build the VS Code extension (.vsix)
build-vscode:
    mkdir -p editors/vscode/build
    docker run --rm -v {{justfile_directory()}}/editors/vscode:/src -w /src node:lts-slim \
        sh -c 'npx --yes @vscode/vsce package --out /src/build/relux.vsix'

# Verify the VS Code extension packages cleanly (no warnings).
check-vscode:
    mkdir -p editors/vscode/build
    docker run --rm -v {{justfile_directory()}}/editors/vscode:/src -w /src node:lts-slim \
        sh -c 'npx --yes @vscode/vsce package --out /src/build/relux-check.vsix 2>&1 | tee /tmp/vsce.log && ! grep -E "WARNING|ERROR" /tmp/vsce.log'

# Validate the agents/ plugin (manifest, skill frontmatter, link resolution).
build-agents:
    ./.scripts/build-agents.sh

# Build tutorial books
build-books: 
    ./.scripts/build-books-sync-book-targets.sh
    mdbook build docs/dsl-tutorial
    mdbook build docs/suite-tutorial
    mdbook build docs/reference

# Build in release mode
build-release: build-viewer
    cargo build --release

## Check targets

# Run all checks: cargo check + clippy + fmt + viewer + ASCII + vscode
check: check-ascii check-clippy check-fmt check-viewer check-vscode

# Fail if any tracked source file contains non-ASCII bytes
check-ascii:
    ./.scripts/check-ascii.sh

# Run clippy check (includes cargo check)
check-clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# Run formatter check
check-fmt:
    rustup run nightly rustfmt --edition 2024 --check $(find crates -name '*.rs')

# Type/Svelte-check the viewer (svelte-check).
check-viewer:
    docker run --rm -v {{justfile_directory()}}/viewer:/src -w /src node:lts-slim \
        sh -c 'npm ci && npm run check'

## Test targets

# Run all tests (unit + e2e)
test: test-unit test-e2e test-viewer

# Run unit + integration tests
test-unit *ARGS:
    cargo test --workspace {{ARGS}}

# Run e2e tests (check then run)
test-e2e:
    cargo run -p relux -- check --manifest tests/Relux.toml
    cargo run -p relux -- run --manifest tests/Relux.toml

# Run viewer unit tests (vitest).
test-viewer:
    docker run --rm -v {{justfile_directory()}}/viewer:/src -w /src node:lts-slim \
        sh -c 'npm ci && npm test'

## Clean targets

# Remove build artifacts
clean:
    cargo clean

# Remove e2e test output logs
clean-logs:
    rm -rf tests/relux/out/run-*
    rm -f tests/relux/out/latest
