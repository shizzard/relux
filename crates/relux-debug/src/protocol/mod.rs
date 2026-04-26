mod handler;
pub mod message;

use std::path::PathBuf;

use jsonrpsee::RpcModule;
use relux_ir::Suite;

pub mod error_code {
    pub const FILE_NOT_FOUND: i32 = -2;
    pub const VERSION_MISMATCH: i32 = -6;
}

// ─── Context ───────────────────────────────────────────────

/// Shared context passed to every RPC handler.
pub struct Context {
    pub suite: Suite,
    /// Absolute path to the suite's `relux/` directory. Used to resolve
    /// wire-format relative paths (e.g. `tests/basic.relux`) into
    /// absolute `FileId`s for source-table lookups.
    pub relux_dir: PathBuf,
}

// ─── MethodRegistry ────────────────────────────────────────

pub struct MethodRegistry {
    module: RpcModule<Context>,
}

impl MethodRegistry {
    pub fn new(suite: Suite, relux_dir: PathBuf) -> Self {
        Self {
            module: RpcModule::new(Context { suite, relux_dir }),
        }
    }

    /// Register session-stage methods (`session/init`).
    pub fn session(mut self) -> Self {
        self.module
            .register_method("session/init", handler::session_init)
            .expect("failed to register session/init");
        self
    }

    /// Register test-select stage methods (`source/get`).
    pub fn test_select(mut self) -> Self {
        self.module
            .register_method("source/get", handler::source_get)
            .expect("failed to register source/get");
        self
    }

    /// Consume the registry and return the built `RpcModule`.
    pub fn build(self) -> RpcModule<Context> {
        self.module
    }
}
