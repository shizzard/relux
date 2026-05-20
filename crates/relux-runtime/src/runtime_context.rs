use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use crate::cancel::CancelToken;
use crate::observe::structured::StructuredLogBuilder;
use relux_core::pure::LayeredEnv;
use relux_ir::IrTimeout;
use relux_ir::Tables;

#[derive(Clone, Debug)]
pub struct ShellConfig {
    pub command: Arc<str>,
    pub prompt: Arc<str>,
    pub default_timeout: IrTimeout,
}

#[derive(Clone)]
pub struct RuntimeContext {
    pub log: StructuredLogBuilder,
    pub shell: ShellConfig,
    pub log_dir: Arc<Path>,
    pub tables: Tables,
    pub env: Arc<LayeredEnv>,
    pub cancel: CancelToken,
    pub test_start: Instant,
    pub flaky_timeout_multiplier: f64,
}
