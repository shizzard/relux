//! Interactive debugger for Relux.
//!
//! Provides a JSON-RPC 2.0 server over WebSocket that implements the
//! Relux Debug Protocol (RDP). The browser-based frontend connects to
//! this server to drive test selection, breakpoint management, stepping,
//! and live shell buffer inspection.

mod log;
pub mod protocol;
mod server;

pub use log::LogLevel;
pub use log::init_tracing;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use relux_core::config::ReluxConfig;
use relux_core::pure::LayeredEnv;
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
pub async fn start_debug_session(
    suite: Suite,
    relux_dir: PathBuf,
    project_root: PathBuf,
    env: Arc<LayeredEnv>,
    relux_config: ReluxConfig,
    multiplier: f64,
    config: DebugConfig,
) {
    init_tracing(config.log_level);

    let env_snapshot = build_env_snapshot(&env, &project_root, &relux_config);

    let addr = SocketAddr::from(([127, 0, 0, 1], config.port));
    let module =
        protocol::MethodRegistry::new(suite, relux_dir, env_snapshot, relux_config, multiplier)
            .session()
            .test_select()
            .events()
            .build();

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

/// Flatten the layered env into a `name → value` map and overlay the
/// run-stable relux internal vars. Per-run / per-test internals
/// (`__RELUX_RUN_ID`, `__RELUX_RUN_ARTIFACTS`, `__RELUX_TEST_*`) are
/// not set here — they materialize at the execution stage. Mirrors the
/// stable subset of `relux-runtime`'s `build_env` so a future refactor
/// can extract a shared helper without behavior drift.
fn build_env_snapshot(
    env: &LayeredEnv,
    project_root: &Path,
    cfg: &ReluxConfig,
) -> HashMap<String, String> {
    let mut snapshot: HashMap<String, String> = env
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    snapshot.insert("__RELUX_SHELL_PROMPT".into(), cfg.shell.prompt.clone());
    snapshot.insert(
        "__RELUX_SUITE_ROOT".into(),
        project_root.display().to_string(),
    );
    if let Ok(exe) = std::env::current_exe() {
        snapshot.insert("__RELUX".into(), exe.display().to_string());
    }
    snapshot
}
