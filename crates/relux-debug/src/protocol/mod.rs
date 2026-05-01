pub mod breakpointable;
mod handler;
pub mod message;
pub mod state;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use jsonrpsee::RpcModule;
use relux_core::config::ReluxConfig;
use relux_ir::Suite;
use tokio::sync::Mutex;
use tokio::sync::broadcast;

use self::message::Event;
use self::state::PreRunInner;
use self::state::Stage;
use self::state::TestSelectInner;
use self::state::build_initial_test_select;

pub mod error_code {
    pub const FILE_NOT_FOUND: i32 = -2;
    pub const VERSION_MISMATCH: i32 = -6;
    pub const TEST_NOT_RUNNABLE: i32 = -7;
    /// `(filename, line)` does not refer to a breakpointable position
    /// in the current `pre_run.source`.
    pub const BREAKPOINT_INVALID: i32 = -8;
}

/// Capacity of the events broadcast channel. Events are small JSON
/// envelopes; this is large enough to hold a brief burst of stage
/// transitions without dropping.
const EVENTS_CHANNEL_CAPACITY: usize = 64;

// ─── Context ───────────────────────────────────────────────

/// Shared context passed to every RPC handler.
///
/// Per-stage state lives in dedicated `Arc<Mutex<...>>` slots so it can
/// outlive the stage marker (enabling backtracking transitions like
/// run → pre-run). See `protocol::state` for the inner structs and
/// lock-order discipline.
pub struct Context {
    pub suite: Suite,
    /// Absolute path to the suite's `relux/` directory. Used to resolve
    /// wire-format relative paths (e.g. `tests/basic.relux`) into
    /// absolute `FileId`s for source-table lookups.
    pub relux_dir: PathBuf,
    /// Snapshot of env vars visible to tests. Built once at session
    /// start: process env + run-stable relux internals (`__RELUX_*`
    /// minus the per-run / per-test ones). Surfaced in pre-run state.
    pub env: HashMap<String, String>,
    /// Parsed `Relux.toml` config. Surfaced in pre-run state and used
    /// elsewhere as the source of truth for shell command, prompt, and
    /// timeouts.
    pub relux_config: ReluxConfig,
    /// Effective debug timeout multiplier (CLI `--timeout-multiplier`).
    /// Surfaced in pre-run state's config block.
    pub multiplier: f64,
    /// Server-pushed events. Subscribers (one per `events/subscribe`
    /// call) consume from a fresh receiver; handlers emit by sending on
    /// this clone.
    pub events: broadcast::Sender<Event>,

    /// Current stage marker. Lock first when also taking a state slot.
    pub stage: Arc<Mutex<Stage>>,
    /// Always populated — test-select is the initial stage.
    pub test_select: Arc<Mutex<TestSelectInner>>,
    /// `Some` after the first successful `test/select`.
    pub pre_run: Arc<Mutex<Option<Box<PreRunInner>>>>,
}

// ─── MethodRegistry ────────────────────────────────────────

pub struct MethodRegistry {
    module: RpcModule<Context>,
}

impl MethodRegistry {
    pub fn new(
        suite: Suite,
        relux_dir: PathBuf,
        env: HashMap<String, String>,
        relux_config: ReluxConfig,
        multiplier: f64,
    ) -> Self {
        let (events, _rx) = broadcast::channel(EVENTS_CHANNEL_CAPACITY);
        let test_select = build_initial_test_select(&suite, &relux_dir);
        Self {
            module: RpcModule::new(Context {
                suite,
                relux_dir,
                env,
                relux_config,
                multiplier,
                events,
                stage: Arc::new(Mutex::new(Stage::TestSelect)),
                test_select: Arc::new(Mutex::new(test_select)),
                pre_run: Arc::new(Mutex::new(None)),
            }),
        }
    }

    /// Register session-stage methods (`session/init`).
    pub fn session(mut self) -> Self {
        self.module
            .register_async_method("session/init", handler::session_init)
            .expect("failed to register session/init");
        self
    }

    /// Register test-select stage methods (`source/get`, `test/select`).
    pub fn test_select(mut self) -> Self {
        self.module
            .register_method("source/get", handler::source_get)
            .expect("failed to register source/get");
        self.module
            .register_async_method("test/select", handler::test_select)
            .expect("failed to register test/select");
        self
    }

    /// Register pre-run stage methods (`breakpoint/set`,
    /// `breakpoint/unset`, `breakpoint/reset`, `breakpoint/list`).
    pub fn pre_run(mut self) -> Self {
        self.module
            .register_async_method("breakpoint/set", handler::breakpoint_set)
            .expect("failed to register breakpoint/set");
        self.module
            .register_async_method("breakpoint/unset", handler::breakpoint_unset)
            .expect("failed to register breakpoint/unset");
        self.module
            .register_async_method("breakpoint/reset", handler::breakpoint_reset)
            .expect("failed to register breakpoint/reset");
        self.module
            .register_async_method("breakpoint/list", handler::breakpoint_list)
            .expect("failed to register breakpoint/list");
        self
    }

    /// Register the events subscription. The client calls
    /// `events/subscribe` once after `session/init` to start receiving
    /// server-pushed events on the WebSocket.
    pub fn events(mut self) -> Self {
        self.module
            .register_subscription(
                "events/subscribe",
                "events/event",
                "events/unsubscribe",
                |_params, pending, ctx, _ext| async move {
                    let sink = match pending.accept().await {
                        Ok(s) => s,
                        Err(_) => return,
                    };
                    let mut rx = ctx.events.subscribe();
                    loop {
                        let event = match rx.recv().await {
                            Ok(ev) => ev,
                            // Lagged or closed → end the subscription.
                            Err(_) => return,
                        };
                        let raw = match serde_json::value::to_raw_value(&event) {
                            Ok(r) => r,
                            Err(_) => return,
                        };
                        if sink.send(raw).await.is_err() {
                            return;
                        }
                    }
                },
            )
            .expect("failed to register events/subscribe");
        self
    }

    /// Consume the registry and return the built `RpcModule`.
    pub fn build(self) -> RpcModule<Context> {
        self.module
    }
}
