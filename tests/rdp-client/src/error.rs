use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to create directory {path}: {source}")]
    CreateDir {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("failed to connect to {url}: {source}")]
    Connect {
        url: String,
        source: tokio_tungstenite::tungstenite::Error,
    },

    #[error("invalid JSON from server: {0}")]
    ServerJson(serde_json::Error),

    #[error("connection closed")]
    ConnectionClosed,

    #[error("WebSocket error: {0}")]
    WebSocket(tokio_tungstenite::tungstenite::Error),

    #[error("failed to read {path}: {source}")]
    ReadFile {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("invalid JSON in {path}: {source}")]
    FileJson {
        path: PathBuf,
        source: serde_json::Error,
    },

    #[error("failed to write {path}: {source}")]
    WriteFile {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("failed to rename {from} to {to}: {source}")]
    Rename {
        from: PathBuf,
        to: PathBuf,
        source: std::io::Error,
    },

    #[error("failed to send: {0}")]
    Send(tokio_tungstenite::tungstenite::Error),

    #[error("failed to read stdin: {0}")]
    Stdin(std::io::Error),

    #[error("unknown method: {0}")]
    UnknownMethod(String),
}
