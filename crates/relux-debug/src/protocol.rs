use jsonrpsee::RpcModule;
use jsonrpsee::types::ErrorObjectOwned;
use relux_ir::Suite;

const VERSION_MISMATCH: i32 = -6;

// ─── MethodRegistry ────────────────────────────────────────

pub struct MethodRegistry {
    module: RpcModule<()>,
}

impl MethodRegistry {
    pub fn new() -> Self {
        Self {
            module: RpcModule::new(()),
        }
    }

    /// Register session-stage methods (`session/init`).
    pub fn session(mut self, suite: &Suite) -> Self {
        let test_count = suite.plans.len();
        self.module
            .register_method("session/init", move |params, _, _| {
                let params: serde_json::Value = params.parse()?;

                let client_version = params.get("version").and_then(|v| v.as_str()).unwrap_or("");

                let server_version = env!("CARGO_PKG_VERSION");

                if client_version != server_version {
                    return Err(ErrorObjectOwned::owned(
                        VERSION_MISMATCH,
                        format!(
                            "version mismatch: client {client_version}, server {server_version}"
                        ),
                        None::<()>,
                    ));
                }

                Ok(serde_json::json!({
                    "server": "relux",
                    "version": server_version,
                    "stage": "test-select",
                    "state": {
                        "project": "debug-stub",
                        "tests": test_count,
                    }
                }))
            })
            .expect("failed to register session/init");
        self
    }

    /// Consume the registry and return the built `RpcModule`.
    pub fn build(self) -> RpcModule<()> {
        self.module
    }
}
