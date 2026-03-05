# Relux — Rust Integration Test Framework

## What is this?

Relux is a Rust reimplementation of [hawk/lux](https://github.com/hawk/lux) (LUcid eXpect scripting) — an Expect-style test automation framework. It controls interactive shell programs by sending input and matching output against expected patterns (regex or literal). The goal is a fast, standalone binary with no Erlang dependency.

## Upstream: hawk/lux

Lux uses a line-prefix DSL (`.lux` files): `!` sends input, `?` matches regex output, `[shell name]` switches shells, `[global var=val]` sets variables. It is written in Erlang/OTP.

## Relux DSL (`.relux` files)

See [docs/semantics.md] and [docs/syntax.md]

## Tech Stack

- **Language:** Rust (edition 2024)
- **Lexer:** `logos` (~0.16)
- **Parser:** `chumsky` (~0.12)

## Project Status

Working prototype — tests can be authored and executed end-to-end.

## Architecture

1. **Lexer** (logos): tokenizes `.relux` files
2. **Parser** (chumsky): tokens → AST
3. **Resolver**: AST → IR, module imports, effect dependency graph
4. **Runtime**: walks IR, spawns PTY shells via tokio, executes send/match with timeouts
5. **Reporter**: test results with ariadne-powered diagnostic output

## Conventions

- File extension: `.relux`
- Examples go in `examples/`
- Follow Rust 2024 edition idioms
