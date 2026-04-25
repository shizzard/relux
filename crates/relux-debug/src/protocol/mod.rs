mod handler;
pub mod message;

use jsonrpsee::RpcModule;
use relux_ir::Suite;

pub mod error_code {
    pub const VERSION_MISMATCH: i32 = -6;
}

// ─── Context ───────────────────────────────────────────────

/// Shared context passed to every RPC handler.
pub struct Context {
    pub suite: Suite,
}

// ─── MethodRegistry ────────────────────────────────────────

pub struct MethodRegistry {
    module: RpcModule<Context>,
}

impl MethodRegistry {
    pub fn new(suite: Suite) -> Self {
        Self {
            module: RpcModule::new(Context { suite }),
        }
    }

    /// Register session-stage methods (`session/init`).
    pub fn session(mut self) -> Self {
        self.module
            .register_method("session/init", handler::session_init)
            .expect("failed to register session/init");
        self
    }

    /// Consume the registry and return the built `RpcModule`.
    pub fn build(self) -> RpcModule<Context> {
        self.module
    }
}
