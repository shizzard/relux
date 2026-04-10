# Default: show available targets
default:
    @just --list

# Build in debug mode
build:
    cargo build

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
books:
    mdbook build docs/dsl-tutorial
    mdbook build docs/suite-tutorial
    mdbook build docs/reference

# Run all tests (unit + e2e)
test: unit e2e

# Run unit tests
unit *ARGS:
    cargo test --workspace --lib {{ARGS}}

# Run relux with arguments
run *ARGS:
    cargo run -p relux -- {{ARGS}}

# Run relux-dbg with arguments
dbg *ARGS:
    cargo run --bin relux-dbg -- {{ARGS}}

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

# Remove build artifacts
clean:
    cargo clean

# Remove e2e test output logs
clean-logs:
    rm -rf tests/relux/out/run-*
    rm -f tests/relux/out/latest
