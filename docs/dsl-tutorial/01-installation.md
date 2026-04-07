# Installation

[Previous: Introduction](00-introduction.md)

Relux is distributed as a single binary with no runtime dependencies.

## Install from crates.io

The simplest way to install Relux is via Cargo:

```text
cargo install relux
```

This downloads, compiles, and installs the `relux` binary into `~/.cargo/bin/`.

## Pre-built binaries

Pre-built binaries for Linux (x86_64) and macOS (aarch64) are available on the [Releases](https://github.com/shizzard/relux/releases) page.

## Building from source

If you prefer to build from source:

```text
git clone https://github.com/shizzard/relux.git
cd relux
cargo build --release
```

The binary will be at `target/release/relux`. You can copy it somewhere on your `PATH`, or run it directly from the build directory.

If you have [just](https://github.com/casey/just) installed, you can also build with:

```text
just release
```

## Prerequisites

All installation methods require a working Rust toolchain. The recommended way to install it is through [rustup](https://rustup.rs/):

```text
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Relux uses Rust edition 2024, which requires **Rust 1.85 or later**. If you already have Rust installed, make sure you are on a recent stable version:

```text
rustup update stable
```

## Verifying the installation

Run `relux` without arguments to confirm the binary works:

```text
relux
```

You should see the help output listing the available subcommands.

---

Next: [Getting Started](02-getting-started.md) — scaffold a project, write and run your first test
