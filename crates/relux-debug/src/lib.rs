//! Interactive debugger for Relux.
//!
//! Provides a JSON-RPC 2.0 server over WebSocket that implements the
//! Relux Debug Protocol (RDP). The browser-based frontend connects to
//! this server to drive test selection, breakpoint management, stepping,
//! and live shell buffer inspection.

use std::fmt;
use std::net::SocketAddr;
use std::str::FromStr;

use jsonrpsee::RpcModule;
use jsonrpsee::server::ServerBuilder;
use jsonrpsee::server::ServerHandle;
use relux_ir::Suite;

// ─── LogLevel ──────────────────────────────────────────────

/// Log level for the debug server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl From<LogLevel> for tracing::Level {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Error => tracing::Level::ERROR,
            LogLevel::Warn => tracing::Level::WARN,
            LogLevel::Info => tracing::Level::INFO,
            LogLevel::Debug => tracing::Level::DEBUG,
            LogLevel::Trace => tracing::Level::TRACE,
        }
    }
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LogLevel::Error => write!(f, "error"),
            LogLevel::Warn => write!(f, "warn"),
            LogLevel::Info => write!(f, "info"),
            LogLevel::Debug => write!(f, "debug"),
            LogLevel::Trace => write!(f, "trace"),
        }
    }
}

impl FromStr for LogLevel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "error" => Ok(LogLevel::Error),
            "warn" => Ok(LogLevel::Warn),
            "info" => Ok(LogLevel::Info),
            "debug" => Ok(LogLevel::Debug),
            "trace" => Ok(LogLevel::Trace),
            other => Err(format!("unknown log level: {other}")),
        }
    }
}

impl clap::ValueEnum for LogLevel {
    fn value_variants<'a>() -> &'a [Self] {
        &[
            LogLevel::Error,
            LogLevel::Warn,
            LogLevel::Info,
            LogLevel::Debug,
            LogLevel::Trace,
        ]
    }

    fn to_possible_value(&self) -> Option<clap::builder::PossibleValue> {
        Some(clap::builder::PossibleValue::new(match self {
            LogLevel::Error => "error",
            LogLevel::Warn => "warn",
            LogLevel::Info => "info",
            LogLevel::Debug => "debug",
            LogLevel::Trace => "trace",
        }))
    }
}

// ─── DebugConfig ───────────────────────────────────────────

/// Configuration for the debug session.
pub struct DebugConfig {
    pub port: u16,
    pub log_level: LogLevel,
}

// ─── Entry Point ───────────────────────────────────────────

/// Start an interactive debug session.
///
/// Initializes tracing, starts a WebSocket server, waits for a client
/// connection, sends a dummy `session/hello` notification, and exits.
pub async fn start_debug_session(suite: &Suite, config: DebugConfig) {
    init_tracing(config.log_level);

    let addr = SocketAddr::from(([127, 0, 0, 1], config.port));
    let listener = match std::net::TcpListener::bind(addr) {
        Ok(l) => {
            l.set_nonblocking(true).expect("failed to set nonblocking");
            l
        }
        Err(e) => {
            tracing::error!(addr = %addr, error = %e, "failed to bind");
            return;
        }
    };
    tracing::info!(addr = %addr, "server listening");

    let server = match ServerBuilder::default().build_from_tcp(listener) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "failed to build server");
            return;
        }
    };

    let mut module = RpcModule::new(());

    // Register a dummy session/hello method (placeholder for the real
    // server-push notification that will be sent on WS connect).
    let test_count = suite.plans.len();
    module
        .register_method("session/hello", move |_params, _, _| {
            serde_json::json!({
                "server": "relux",
                "version": env!("CARGO_PKG_VERSION"),
                "stage": "test-select",
                "state": {
                    "project": "debug-stub",
                    "tests": test_count,
                }
            })
        })
        .expect("failed to register method");

    let handle: ServerHandle = server.start(module);

    tracing::info!("waiting for connection");

    // Wait for the server to finish (Ctrl+C to stop)
    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen for ctrl+c");

    tracing::info!("shutting down");
    handle.stop().expect("failed to stop server");
    handle.stopped().await;
}

fn init_tracing(level: LogLevel) {
    use tracing_subscriber::EnvFilter;
    use tracing_subscriber::fmt::FormatEvent;
    use tracing_subscriber::fmt::FormattedFields;
    use tracing_subscriber::fmt::format::Writer;
    use tracing_subscriber::registry::LookupSpan;

    struct RdpFormatter;

    impl<S, N> FormatEvent<S, N> for RdpFormatter
    where
        S: tracing::Subscriber + for<'a> LookupSpan<'a>,
        N: for<'a> tracing_subscriber::fmt::FormatFields<'a> + 'static,
    {
        fn format_event(
            &self,
            ctx: &tracing_subscriber::fmt::FmtContext<'_, S, N>,
            mut writer: Writer<'_>,
            event: &tracing::Event<'_>,
        ) -> fmt::Result {
            let level = event.metadata().level().as_str().to_ascii_lowercase();
            write!(writer, "[{level}]")?;

            // Write span fields
            if let Some(scope) = ctx.event_scope() {
                for span in scope.from_root() {
                    let extensions = span.extensions();
                    if let Some(fields) = extensions.get::<FormattedFields<N>>()
                        && !fields.is_empty()
                    {
                        write!(writer, " {fields}")?;
                    }
                }
            }

            // Write event fields
            write!(writer, " ")?;
            ctx.field_format().format_fields(writer.by_ref(), event)?;
            writeln!(writer)
        }
    }

    let filter = EnvFilter::new(level.to_string());
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .event_format(RdpFormatter)
        .init();
}
