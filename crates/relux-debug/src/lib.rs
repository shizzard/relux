//! Interactive debugger for Relux.
//!
//! Provides a JSON-RPC 2.0 server over WebSocket that implements the
//! Relux Debug Protocol (RDP). The browser-based frontend connects to
//! this server to drive test selection, breakpoint management, stepping,
//! and live shell buffer inspection.

mod log;
mod protocol;
mod server;

pub use log::LogLevel;
pub use log::init_tracing;

use std::net::SocketAddr;

use relux_ir::Suite;

// ─── DebugConfig ───────────────────────────────────────────

/// Configuration for the debug session.
pub struct DebugConfig {
    pub port: u16,
    pub log_level: LogLevel,
}

// ─── Entry Point ───────────────────────────────────────────

/// Start an interactive debug session.
///
/// Initializes tracing, starts a WebSocket server, and blocks until
/// the user sends Ctrl+C.
pub async fn start_debug_session(suite: &Suite, config: DebugConfig) {
    init_tracing(config.log_level);

    let addr = SocketAddr::from(([127, 0, 0, 1], config.port));
    let module = protocol::MethodRegistry::new().session(suite).build();

    let server = match server::DebugServer::start(addr, module) {
        Ok(s) => {
            tracing::info!(addr = %addr, "server listening");
            s
        }
        Err(e) => {
            tracing::error!(error = %e, "failed to start server");
            return;
        }
    };

    tracing::info!("waiting for connection");
    server.wait_for_shutdown().await;
}
