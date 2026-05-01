use std::path::Path;

use relux_debug::protocol::message::BreakpointListRequest;
use relux_debug::protocol::message::BreakpointResetRequest;
use relux_debug::protocol::message::BreakpointSetRequest;
use relux_debug::protocol::message::BreakpointUnsetRequest;
use relux_debug::protocol::message::SessionInitRequest;
use relux_debug::protocol::message::SourceGetRequest;
use relux_debug::protocol::message::TestSelectRequest;

use crate::error::Error;
use crate::message;

pub fn cmd_request(
    method: &str,
    id: u64,
    filename: Option<&str>,
    test: Option<&str>,
    line: Option<usize>,
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
        "test/select" => {
            let filename = filename.ok_or_else(|| Error::MissingArg {
                arg: "filename",
                method: method.to_string(),
            })?;
            let test = test.ok_or_else(|| Error::MissingArg {
                arg: "test",
                method: method.to_string(),
            })?;
            test_select_envelope(id, filename, test)
        }
        "breakpoint/set" => {
            let filename = filename.ok_or_else(|| Error::MissingArg {
                arg: "filename",
                method: method.to_string(),
            })?;
            let line = line.ok_or_else(|| Error::MissingArg {
                arg: "line",
                method: method.to_string(),
            })?;
            breakpoint_set_envelope(id, filename, line)
        }
        "breakpoint/unset" => {
            let filename = filename.ok_or_else(|| Error::MissingArg {
                arg: "filename",
                method: method.to_string(),
            })?;
            let line = line.ok_or_else(|| Error::MissingArg {
                arg: "line",
                method: method.to_string(),
            })?;
            breakpoint_unset_envelope(id, filename, line)
        }
        "breakpoint/reset" => breakpoint_reset_envelope(id),
        "breakpoint/list" => breakpoint_list_envelope(id),
        "events/subscribe" => events_subscribe_envelope(id),
        _ => return Err(Error::UnknownMethod(method.to_string())),
    };

    // Include the JSON-RPC id in the filename so multiple requests of
    // the same method (e.g. several `breakpoint/set` calls in one test)
    // produce distinct files instead of clobbering each other.
    let filename = format!("{}-{}.json", message::sanitize_method(method), id);
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

fn test_select_envelope(id: u64, filename: &str, test: &str) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "test/select",
        "params": TestSelectRequest {
            filename: filename.to_string(),
            test: test.to_string(),
        }
    })
}

fn events_subscribe_envelope(id: u64) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "events/subscribe",
        "params": []
    })
}

fn breakpoint_set_envelope(id: u64, filename: &str, line: usize) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "breakpoint/set",
        "params": BreakpointSetRequest {
            filename: filename.to_string(),
            line,
        }
    })
}

fn breakpoint_unset_envelope(id: u64, filename: &str, line: usize) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "breakpoint/unset",
        "params": BreakpointUnsetRequest {
            filename: filename.to_string(),
            line,
        }
    })
}

fn breakpoint_reset_envelope(id: u64) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "breakpoint/reset",
        "params": BreakpointResetRequest::default()
    })
}

fn breakpoint_list_envelope(id: u64) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "breakpoint/list",
        "params": BreakpointListRequest::default()
    })
}
