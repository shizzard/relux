/**
 * `events.json` schema version this viewer build understands. Must match
 * the `schema_version` field on `StructuredLog` emitted by the runtime
 * (constant `SCHEMA_VERSION` in `crates/relux-runtime/src/observe/structured/mod.rs`).
 * Bump in lockstep with the Rust-side constant on any backwards-incompatible
 * change to the on-disk shape.
 */
export const EXPECTED_SCHEMA_VERSION = 1;
