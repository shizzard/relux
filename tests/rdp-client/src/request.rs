use std::path::Path;

use relux_debug::protocol::message::SessionInitRequest;
use relux_debug::protocol::message::SourceGetRequest;

use crate::error::Error;
use crate::message;

pub fn cmd_request(
    method: &str,
    id: u64,
    filename: Option<&str>,
    dir: &Path,
) -> Result<(), Error> {
    std::fs::create_dir_all(dir).map_err(|e| Error::CreateDir {
        path: dir.to_path_buf(),
        source: e,
    })?;

    let json = match method {
        "session/init" => session_init_envelope(id),
        "source/get" => {
            let filename = filename.ok_or_else(|| Error::MissingArg {
                arg: "filename",
                method: method.to_string(),
            })?;
            source_get_envelope(id, filename)
        }
        _ => return Err(Error::UnknownMethod(method.to_string())),
    };

    let filename = format!("{}.json", message::sanitize_method(method));
    message::write_message_file(dir, &filename, &json)?;
    println!("file written: {filename}");
    Ok(())
}

fn session_init_envelope(id: u64) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "session/init",
        "params": SessionInitRequest {
            client: "rdp-client".to_string(),
            version: relux_core::VERSION.to_string(),
        }
    })
}

fn source_get_envelope(id: u64, filename: &str) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "source/get",
        "params": SourceGetRequest {
            filename: filename.to_string(),
        }
    })
}
