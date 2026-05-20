//! Relux language definition for highlight.js. The canonical grammar
//! lives in the sibling `highlight-relux.js` file; it is embedded into
//! `event.html` via `event_html.rs` and copied into each mdBook
//! directory by the `just books` target.

pub const HLJS_RELUX_INIT: &str = include_str!("highlight-relux.js");
