//! Interactive debugger for Relux.
//!
//! Provides a JSON-RPC 2.0 server over WebSocket that implements the
//! Relux Debug Protocol (RDP). The browser-based frontend connects to
//! this server to drive test selection, breakpoint management, stepping,
//! and live shell buffer inspection.
