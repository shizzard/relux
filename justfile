# Default: show available targets
default:
    @just --list

# Build in debug mode
build:
    cargo build

# Build in release mode
release:
    cargo build --release

# Run cargo check
check:
    cargo check

# Run all tests (unit + e2e)
test: unit e2e

# Run unit tests
unit *ARGS:
    cargo test --lib {{ARGS}}

# Run e2e tests (check then run)
e2e *ARGS:
    cargo run -- check --manifest tests/Relux.toml {{ARGS}}
    cargo run -- run --manifest tests/Relux.toml {{ARGS}}

# Analyze run history
history *ARGS:
    cargo run -- history --manifest tests/Relux.toml {{ARGS}}

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
