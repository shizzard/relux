# Installation

[Previous: Introduction](00-introduction.md)

Relux is distributed as a single binary with no runtime dependencies. For now, the only installation method is building from source. This page will be expanded as more distribution options become available.

## Prerequisites

You need a working Rust toolchain. The recommended way to install it is through [rustup](https://rustup.rs/):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Relux uses Rust edition 2024, which requires **Rust 1.85 or later**. If you already have Rust installed, make sure you are on a recent stable version:

```bash
rustup update stable
```

## Building from source

Clone the repository and build in release mode:

```bash
git clone https://github.com/spawnlink/relux.git
cd relux
cargo build --release
```

The binary will be at `target/release/relux`. You can copy it somewhere on your `PATH`, or run it directly from the build directory.

If you have [just](https://github.com/casey/just) installed, you can also build with:

```bash
just release
```

## Verifying the installation

Run `relux` without arguments to confirm the binary works:

```bash
relux
```

You should see the help output listing the available subcommands.

---

Next: [Getting Started](02-getting-started.md) — scaffold a project, write and run your first test
