use std::path::Path;

use crate::error::Error;

// ─── Message types ───────────────────────────────────────────

pub enum MessageType {
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

// ─── Helpers ─────────────────────────────────────────────────

/// Replace `/` with `--` to make a method name filesystem-safe.
pub fn sanitize_method(method: &str) -> String {
    method.replace('/', "--")
}

/// Build a message filename: `00001-event-session--hello.json`
pub fn message_filename(counter: u32, msg_type: MessageType, method: &str) -> String {
    format!(
        "{:05}-{}-{}.json",
        counter,
        msg_type.as_str(),
        sanitize_method(method),
    )
}

/// Write pretty-printed JSON to a file in the working directory.
pub fn write_message_file(
    dir: &Path,
    filename: &str,
    json: &serde_json::Value,
) -> Result<(), Error> {
    let path = dir.join(filename);
    let pretty = serde_json::to_string_pretty(json).map_err(Error::ServerJson)?;
    std::fs::write(&path, pretty).map_err(|e| Error::WriteFile { path, source: e })
}

/// Extract the JSON-RPC method name from a parsed message.
pub fn extract_method(json: &serde_json::Value) -> Option<&str> {
    json.get("method").and_then(|v| v.as_str())
}

/// Extract the JSON-RPC `id` field as a string key for tracking.
pub fn extract_id(json: &serde_json::Value) -> Option<String> {
    json.get("id").map(|v| v.to_string())
}
