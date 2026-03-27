use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use tokio_util::sync::CancellationToken;

use crate::dsl::resolver::ir::IrTimeout;
use crate::dsl::resolver::ir::Tables;
use crate::pure::Env;
use crate::runtime::observe::event_sink::EventSink;
use crate::runtime::observe::shell_log::ShellLogger;

#[derive(Clone, Debug)]
pub struct ShellConfig {
    pub command: Arc<str>,
    pub prompt: Arc<str>,
    pub default_timeout: IrTimeout,
}

#[derive(Clone)]
pub struct RuntimeContext {
    pub events: EventSink,
    pub shell: ShellConfig,
    pub log_dir: Arc<Path>,
    pub tables: Tables,
    pub env: Arc<Env>,
    pub cancel: CancellationToken,
    pub test_start: Instant,
}

impl RuntimeContext {
    pub fn create_shell_logger(&self, name: &str) -> std::io::Result<ShellLogger> {
        ShellLogger::create(&self.log_dir, name, self.test_start)
    }
}
