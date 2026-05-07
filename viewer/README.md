# relux-viewer

Svelte SPA that renders a Relux test report (`events.json` produced by `relux-runtime`).

This is a scaffold; the full timeline UI is built across follow-up commits. The current placeholder loads the schema-typed JSON and shows a summary plus a raw dump.

## Build

The vendored, gzipped IIFE bundle lives at `vendor/relux-viewer.js.gz` (committed). It is embedded into the `relux-runtime` binary via `include_bytes!`, so a fresh `cargo build` does not require any JS toolchain.

To regenerate the vendored bundle (after editing anything under `viewer/`):

```sh
just viewer-build
```

That recipe (a) regenerates the TS schema from the Rust types via ts-rs, (b) runs `npm ci && npm run build` inside the `node:lts-slim` Docker image, and (c) copies the gzipped bundle into `vendor/`.

A pre-commit hook (installed once via `just install-hooks`) and a CI step both verify that the vendored bundle stays in sync with `viewer/` sources.

## TypeScript schema

Types under `src/types/` are generated from Rust by `ts-rs`, driven by the `ts-export` cargo feature on `relux-runtime`. They are regenerated as the first step of `just viewer-build` and gitignored.
