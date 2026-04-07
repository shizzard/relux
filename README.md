# Relux

Expect-style integration testing for interactive shell programs.

[![CI](https://github.com/shizzard/relux/actions/workflows/ci.yml/badge.svg)](https://github.com/shizzard/relux/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE.md)

```relux
test "service healthcheck" {
    let DATABASE_PORT = available_port()
    let SERVICE_PORT = available_port()
    
    start Service as service { DATABASE_PORT, SERVICE_PORT }
    
    shell test {
        > curl localhost:${SERVICE_PORT}/status
        <? ^status: running
    }
}
```

Relux sends input to PTY shells and matches output against regex or literal patterns with timeouts. Tests read like a transcript of a shell session — send a command, match the response, repeat.

## Features

- **Pattern matching** — literal (`<=`) and regex (`<?`) with capture groups
- **Timeouts** — per-operator, per-test, and suite-level, with tolerance and assertion modes
- **Effects** — declarative test infrastructure with dependency graphs and automatic teardown
- **Parallel execution** — run tests concurrently with isolated environments
- **Functions and modules** — extract reusable logic, organize suites across files
- **Fail patterns** — continuous background monitoring for errors
- **HTML reports** — rich test output with shell I/O logs
- **Single binary** — no runtime dependencies

## Installation

Build from source:

```
git clone https://github.com/shizzard/relux.git
cd relux
cargo build --release
```

The binary is at `target/release/relux`. Pre-built binaries are available on the [Releases](https://github.com/shizzard/relux/releases) page.

## Quick start

```
relux new                        # scaffold a project
$EDITOR relux/tests/hello.relux  # write a test
relux run                        # run all tests
```

## Documentation

- [DSL Tutorial](https://shizzard.github.io/relux/latest/dsl-tutorial/) — learn the Relux language from scratch
- [Suite Tutorial](https://shizzard.github.io/relux/latest/suite-tutorial/) — build a real-world test suite with shared infrastructure
- [Syntax Reference](docs/syntax.md)
- [Semantics Reference](docs/semantics.md)
