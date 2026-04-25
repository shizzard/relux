use std::collections::HashMap;
use std::path::Path;

use futures_util::{SinkExt, StreamExt};
use tokio::io::AsyncBufReadExt;
use tokio_tungstenite::tungstenite::Message;

use crate::error::Error;
use crate::message::{self, MessageType};

const PROMPT: &str = "rdp> ";

pub async fn cmd_connect(host: &str, port: u16, dir: &Path) -> Result<(), Error> {
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

                        let (msg_type, method_name) = if let Some(method) = message::extract_method(&json) {
                            if json.get("id").is_some() {
                                (MessageType::Request, method.to_string())
                            } else {
                                (MessageType::Event, method.to_string())
                            }
                        } else if let Some(id_key) = message::extract_id(&json) {
                            let method = pending_methods
                                .remove(&id_key)
                                .unwrap_or_else(|| "unknown".to_string());
                            (MessageType::Response, method)
                        } else {
                            (MessageType::Event, "unknown".to_string())
                        };

                        let filename = message::message_filename(counter, msg_type, &method_name);
                        message::write_message_file(dir, &filename, &json)?;
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

                        if let (Some(id_key), Some(method)) = (message::extract_id(&json), message::extract_method(&json)) {
                            pending_methods.insert(id_key, method.to_string());
                        }

                        counter += 1;
                        let method = message::extract_method(&json).unwrap_or("unknown");
                        let out_filename = message::message_filename(counter, MessageType::Request, method);
                        let new_path = dir.join(&out_filename);
                        std::fs::rename(&path, &new_path).map_err(|e| Error::Rename {
                            from: path,
                            to: new_path,
                            source: e,
                        })?;
                        eprintln!("{out_filename}");

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
