use std::collections::HashMap;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use futures_util::{SinkExt, StreamExt};
use tokio::io::AsyncBufReadExt;
use tokio_tungstenite::tungstenite::Message;

const PROMPT: &str = "rdp> ";

// ─── Error ────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
enum Error {
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

    #[error("failed to send: {0}")]
    Send(tokio_tungstenite::tungstenite::Error),

    #[error("failed to read stdin: {0}")]
    Stdin(std::io::Error),
}

// ─── CLI ──────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "rdp-client", about = "Relux Debug Protocol test client")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Connect to an RDP server and enter the message loop
    Connect {
        /// Server host
        #[arg(long, default_value = "127.0.0.1")]
        host: String,

        /// Server port
        #[arg(long, default_value_t = 9377)]
        port: u16,

        /// Working directory for message files (relative to cwd)
        #[arg(long)]
        dir: PathBuf,
    },
}

// ─── Message file helpers ─────────────────────────────────

enum MessageType {
    Event,
    Request,
    Response,
}

impl MessageType {
    fn as_str(&self) -> &'static str {
        match self {
            MessageType::Event => "event",
            MessageType::Request => "request",
            MessageType::Response => "response",
        }
    }
}

/// Replace `/` with `--` to make a method name filesystem-safe.
fn sanitize_method(method: &str) -> String {
    method.replace('/', "--")
}

/// Build a message filename: `00001-event-session--hello.json`
fn message_filename(counter: u32, msg_type: MessageType, method: &str) -> String {
    format!(
        "{:05}-{}-{}.json",
        counter,
        msg_type.as_str(),
        sanitize_method(method),
    )
}

/// Write pretty-printed JSON to a file in the working directory.
fn write_message_file(
    dir: &Path,
    filename: &str,
    json: &serde_json::Value,
) -> Result<(), Error> {
    let path = dir.join(filename);
    let pretty = serde_json::to_string_pretty(json).map_err(Error::ServerJson)?;
    std::fs::write(&path, pretty).map_err(|e| Error::WriteFile {
        path,
        source: e,
    })
}

/// Extract the JSON-RPC method name from a parsed message.
fn extract_method(json: &serde_json::Value) -> Option<&str> {
    json.get("method").and_then(|v| v.as_str())
}

/// Extract the JSON-RPC `id` field as a string key for tracking.
fn extract_id(json: &serde_json::Value) -> Option<String> {
    json.get("id").map(|v| v.to_string())
}

// ─── Entry point ──────────────────────────────────────────

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Connect { host, port, dir } => cmd_connect(&host, port, &dir).await,
    };

    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

async fn cmd_connect(host: &str, port: u16, dir: &Path) -> Result<(), Error> {
    std::fs::create_dir_all(dir).map_err(|e| Error::CreateDir {
        path: dir.to_path_buf(),
        source: e,
    })?;

    let url = format!("ws://{host}:{port}");
    let (ws_stream, _) =
        tokio_tungstenite::connect_async(&url)
            .await
            .map_err(|e| Error::Connect {
                url: url.clone(),
                source: e,
            })?;

    let (mut ws_sink, mut ws_stream) = ws_stream.split();

    let mut counter: u32 = 0;
    let mut pending_methods: HashMap<String, String> = HashMap::new();

    let stdin = tokio::io::stdin();
    let mut lines = tokio::io::BufReader::new(stdin).lines();

    eprint!("{PROMPT}");

    loop {
        tokio::select! {
            msg = ws_stream.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        let json: serde_json::Value =
                            serde_json::from_str(&text).map_err(Error::ServerJson)?;

                        counter += 1;

                        let (msg_type, method_name) = if let Some(method) = extract_method(&json) {
                            if json.get("id").is_some() {
                                (MessageType::Request, method.to_string())
                            } else {
                                (MessageType::Event, method.to_string())
                            }
                        } else if let Some(id_key) = extract_id(&json) {
                            let method = pending_methods
                                .remove(&id_key)
                                .unwrap_or_else(|| "unknown".to_string());
                            (MessageType::Response, method)
                        } else {
                            (MessageType::Event, "unknown".to_string())
                        };

                        let filename = message_filename(counter, msg_type, &method_name);
                        write_message_file(dir, &filename, &json)?;
                        eprintln!("{filename}");
                        eprint!("{PROMPT}");
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        return Err(Error::ConnectionClosed);
                    }
                    Some(Ok(_)) => {
                        // Ignore ping/pong/binary frames.
                    }
                    Some(Err(e)) => {
                        return Err(Error::WebSocket(e));
                    }
                }
            }

            line = lines.next_line() => {
                match line {
                    Ok(Some(filename)) => {
                        let filename = filename.trim().to_string();
                        if filename.is_empty() {
                            eprint!("{PROMPT}");
                            continue;
                        }

                        let path = dir.join(&filename);
                        let content = std::fs::read_to_string(&path).map_err(|e| {
                            Error::ReadFile { path: path.clone(), source: e }
                        })?;

                        let json: serde_json::Value =
                            serde_json::from_str(&content).map_err(|e| {
                                Error::FileJson { path: path.clone(), source: e }
                            })?;

                        if let (Some(id_key), Some(method)) = (extract_id(&json), extract_method(&json)) {
                            pending_methods.insert(id_key, method.to_string());
                        }

                        counter += 1;
                        let method = extract_method(&json).unwrap_or("unknown");
                        let out_filename = message_filename(counter, MessageType::Request, method);
                        write_message_file(dir, &out_filename, &json)?;

                        ws_sink
                            .send(Message::Text(content.into()))
                            .await
                            .map_err(Error::Send)?;

                        eprint!("{PROMPT}");
                    }
                    Ok(None) => {
                        return Ok(());
                    }
                    Err(e) => {
                        return Err(Error::Stdin(e));
                    }
                }
            }
        }
    }
}
