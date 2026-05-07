# relux-viewer

Svelte SPA that renders a Relux test report (`events.json` produced by `relux-runtime`).

This is a scaffold; the full timeline UI is built across follow-up commits. The current placeholder loads the schema-typed JSON and shows a summary plus a raw dump.

## Develop

All `npm` work is run inside the `node:lts-slim` Docker image (matching the
`editors/vscode` pattern); local node is not required.

```sh
just viewer-build    # produces dist/relux-viewer.js + dist/relux-viewer.js.gz
```

To preview the placeholder UI, open `viewer/dist/relux-viewer.js` from an
HTML page that pre-populates `window.RELUX_DATA` (or load the build output
alongside an `events.json` once commit 8 wires the report writer).

## TypeScript schema

Types under `src/types/` are generated from Rust by `ts-rs`. Regenerate with:

```sh
just viewer-types
```

Driven by the `ts-export` cargo feature on `relux-runtime`. The `src/types/` directory is gitignored.

## Fixture

`fixtures/sample.events.json` is a real `events.json` captured from one of the project's e2e tests. It serves as a reference example of the schema shape; swap it by running `relux` against any module and copying the per-test `events.json`.
