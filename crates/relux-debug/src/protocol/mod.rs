mod handler;
pub mod message;

use std::path::PathBuf;
use std::sync::Arc;

use jsonrpsee::RpcModule;
use relux_ir::Suite;
use tokio::sync::Mutex;
use tokio::sync::broadcast;

use self::message::Event;
use self::message::PreRunState;

pub mod error_code {
    pub const FILE_NOT_FOUND: i32 = -2;
    pub const VERSION_MISMATCH: i32 = -6;
    pub const TEST_NOT_RUNNABLE: i32 = -7;
}

/// Capacity of the events broadcast channel. Events are small JSON
/// envelopes; this is large enough to hold a brief burst of stage
/// transitions without dropping.
const EVENTS_CHANNEL_CAPACITY: usize = 64;

// ─── SessionStage ──────────────────────────────────────────

/// Mutable session stage. Mutated by stage-transitioning handlers
/// (e.g. `test/select` moves `TestSelect → PreRun`). Always under
/// the `Context.session` mutex.
pub enum SessionStage {
    TestSelect,
    PreRun { state: PreRunState },
}

// ─── Context ───────────────────────────────────────────────

/// Shared context passed to every RPC handler.
pub struct Context {
    pub suite: Suite,
    /// Absolute path to the suite's `relux/` directory. Used to resolve
    /// wire-format relative paths (e.g. `tests/basic.relux`) into
    /// absolute `FileId`s for source-table lookups.
    pub relux_dir: PathBuf,
    /// Current session stage. Stage-transitioning handlers update this
    /// under the mutex; `session/init` reads it to assemble its response.
    pub session: Arc<Mutex<SessionStage>>,
    /// Server-pushed events. Subscribers (one per `events/subscribe`
    /// call) consume from a fresh receiver; handlers emit by sending on
    /// this clone.
    pub events: broadcast::Sender<Event>,
}

// ─── MethodRegistry ────────────────────────────────────────

pub struct MethodRegistry {
    module: RpcModule<Context>,
}

impl MethodRegistry {
    pub fn new(suite: Suite, relux_dir: PathBuf) -> Self {
        let (events, _rx) = broadcast::channel(EVENTS_CHANNEL_CAPACITY);
        Self {
            module: RpcModule::new(Context {
                suite,
                relux_dir,
                session: Arc::new(Mutex::new(SessionStage::TestSelect)),
                events,
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
