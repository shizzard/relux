use std::net::SocketAddr;

use jsonrpsee::RpcModule;
use jsonrpsee::server::ServerBuilder;
use jsonrpsee::server::ServerHandle;

// ─── ServerError ───────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("failed to bind: {0}")]
    Bind(std::io::Error),

    #[error("failed to build server: {0}")]
    Build(std::io::Error),
}

// ─── DebugServer ───────────────────────────────────────────

pub struct DebugServer {
    handle: ServerHandle,
}

impl DebugServer {
    /// Bind to `addr`, build the jsonrpsee server, and start serving `module`.
    pub fn start<C: Send + Sync + 'static>(
        addr: SocketAddr,
        module: RpcModule<C>,
    ) -> Result<Self, ServerError> {
        let listener = std::net::TcpListener::bind(addr).map_err(ServerError::Bind)?;
        listener
            .set_nonblocking(true)
            .expect("failed to set nonblocking");

        let server = ServerBuilder::default()
            .build_from_tcp(listener)
            .map_err(ServerError::Build)?;

        let handle = server.start(module);
        Ok(Self { handle })
    }

    /// Wait for Ctrl+C, then shut down gracefully.
    pub async fn wait_for_shutdown(self) {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to listen for ctrl+c");

        tracing::info!("shutting down");
        self.handle.stop().expect("failed to stop server");
        self.handle.stopped().await;
    }
}
