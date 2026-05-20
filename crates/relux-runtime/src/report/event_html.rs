//! Per-test `event.html` emitter.
//!
//! Writes a single self-contained HTML file beside `events.json`. Four
//! payloads — the `StructuredLog` JSON, the highlight.js core, the Relux
//! hljs grammar, and the Svelte viewer bundle — are each gzipped and
//! base64-encoded into `<script type="application/octet-stream">` tags.
//! A small bootstrap `<script>` decompresses them with the browser-native
//! `DecompressionStream`, sets `window.RELUX_DATA`, and synchronously
//! evaluates the three JS payloads in order (hljs core → Relux grammar →
//! viewer bundle).
//!
//! No `fetch`, no CORS, opens directly via `file://`. Requires a browser
//! with `DecompressionStream` (Chrome 80+ / Firefox 113+ / Safari 16.4+);
//! older browsers see a one-line message and nothing else.

use std::io::Write;
use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use flate2::Compression;
use flate2::write::GzEncoder;

use crate::observe::structured::StructuredLog;
use crate::viewer;

const PREFIX: &str = "<!doctype html>\n\
    <html lang=\"en\">\n\
    <head><meta charset=\"utf-8\"><title>Relux test report</title>\
    <style>html,body{margin:0;padding:0}</style></head>\n\
    <body><div id=\"app\"></div>\n";

// Bootstrap script. The IDs `d` / `h` / `i` / `v` match the four payload
// tag IDs emitted below. Order of execution mirrors the prior inline-script
// layout (data first so the viewer can read `window.RELUX_DATA` at mount).
const BOOTSTRAP: &str = "<script>\n\
(async () => {\n\
  if (!window.DecompressionStream) {\n\
    document.body.textContent = \
\"This report needs Chrome 80+, Firefox 113+, or Safari 16.4+.\";\n\
    return;\n\
  }\n\
  const unzip = async id => {\n\
    const b64 = document.getElementById(id).textContent;\n\
    const bin = Uint8Array.from(atob(b64), c => c.charCodeAt(0));\n\
    const s = new Blob([bin]).stream().pipeThrough(new DecompressionStream(\"gzip\"));\n\
    return await new Response(s).text();\n\
  };\n\
  const runJs = code => {\n\
    const s = document.createElement(\"script\");\n\
    s.textContent = code;\n\
    document.head.appendChild(s);\n\
  };\n\
  window.RELUX_DATA = JSON.parse(await unzip(\"d\"));\n\
  runJs(await unzip(\"h\"));\n\
  runJs(await unzip(\"i\"));\n\
  runJs(await unzip(\"v\"));\n\
})();\n\
</script>\n";

const SUFFIX: &str = "</body></html>\n";

pub fn write(log_dir: &Path, structured: &StructuredLog) -> std::io::Result<()> {
    let html = render(structured)?;
    std::fs::write(log_dir.join("event.html"), html)
}

fn render(structured: &StructuredLog) -> std::io::Result<String> {
    let json = serde_json::to_vec(structured).map_err(std::io::Error::other)?;

    let data_b64 = encode_gz(&json)?;
    let hljs_b64 = BASE64.encode(viewer::hljs_gz());
    let init_b64 = BASE64.encode(viewer::hljs_init_gz());
    let bundle_b64 = BASE64.encode(viewer::bundle_gz());

    let mut html = String::with_capacity(
        PREFIX.len()
            + payload_tag_len("d", data_b64.len())
            + payload_tag_len("h", hljs_b64.len())
            + payload_tag_len("i", init_b64.len())
            + payload_tag_len("v", bundle_b64.len())
            + BOOTSTRAP.len()
            + SUFFIX.len(),
    );
    html.push_str(PREFIX);
    push_payload_tag(&mut html, "d", &data_b64);
    push_payload_tag(&mut html, "h", &hljs_b64);
    push_payload_tag(&mut html, "i", &init_b64);
    push_payload_tag(&mut html, "v", &bundle_b64);
    html.push_str(BOOTSTRAP);
    html.push_str(SUFFIX);
    Ok(html)
}

/// Gzip-compress raw bytes, then base64-encode the result.
fn encode_gz(bytes: &[u8]) -> std::io::Result<String> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(bytes)?;
    let gz = encoder.finish()?;
    Ok(BASE64.encode(gz))
}

fn push_payload_tag(html: &mut String, id: &str, b64: &str) {
    html.push_str("<script type=\"application/octet-stream\" id=\"");
    html.push_str(id);
    html.push_str("\">");
    html.push_str(b64);
    html.push_str("</script>\n");
}

fn payload_tag_len(id: &str, b64_len: usize) -> usize {
    // <script type="application/octet-stream" id="X">...</script>\n
    "<script type=\"application/octet-stream\" id=\"".len()
        + id.len()
        + "\">".len()
        + b64_len
        + "</script>\n".len()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::io::Read;

    use base64::Engine;
    use base64::engine::general_purpose::STANDARD as BASE64;
    use flate2::read::GzDecoder;

    use super::*;
    use crate::observe::structured::EnvInfo;
    use crate::observe::structured::StructuredLog;
    use crate::observe::structured::TestInfo;
    use crate::observe::structured::TestOutcome;

    fn sample_log(test_name: &str) -> StructuredLog {
        StructuredLog {
            schema_version: crate::observe::structured::SCHEMA_VERSION,
            info: TestInfo {
                name: test_name.to_string(),
                path: "tests/foo.relux".to_string(),
                duration_ms: 42,
            },
            outcome: TestOutcome::Pass,
            env: EnvInfo::default(),
            shells: HashMap::new(),
            spans: HashMap::new(),
            events: Vec::new(),
            buffer_events: Vec::new(),
            sources: HashMap::new(),
            artifacts: Vec::new(),
        }
    }

    /// Extract the body of the `<script type="application/octet-stream"
    /// id="...">` tag with the given id, then base64-decode + gunzip its
    /// contents into raw bytes. Used to assert on payloads round-trip.
    fn decode_payload(html: &str, id: &str) -> Vec<u8> {
        let opener = format!("<script type=\"application/octet-stream\" id=\"{id}\">");
        let start = html.find(&opener).expect("payload tag opener missing") + opener.len();
        let end = html[start..]
            .find("</script>")
            .expect("payload tag closer missing")
            + start;
        let b64 = &html[start..end];
        let gz = BASE64.decode(b64).expect("payload base64 decode failed");
        let mut out = Vec::new();
        GzDecoder::new(gz.as_slice())
            .read_to_end(&mut out)
            .expect("payload gunzip failed");
        out
    }

    #[test]
    fn html_inlines_payload_and_bundle_entry_hook() {
        let html = render(&sample_log("hello-world")).unwrap();
        let data: serde_json::Value = serde_json::from_slice(&decode_payload(&html, "d")).unwrap();
        assert_eq!(data["info"]["name"], "hello-world");

        let bundle = decode_payload(&html, "v");
        // The Svelte bundle reads `window.RELUX_DATA` at mount time; if this
        // string disappears, decompression silently dropped the bundle body.
        let bundle_str = std::str::from_utf8(&bundle).unwrap();
        assert!(bundle_str.contains("RELUX_DATA"));
    }

    #[test]
    fn render_inlines_artifacts_into_data_payload() {
        use crate::observe::structured::ArtifactEntry;
        let mut log = sample_log("with-artifacts");
        log.artifacts = vec![
            ArtifactEntry {
                path: "out.txt".to_string(),
                size: 12,
                mime: Some("text/plain".to_string()),
            },
            ArtifactEntry {
                path: "sut/error.log".to_string(),
                size: 4096,
                mime: None,
            },
        ];
        let html = render(&log).unwrap();
        let data: serde_json::Value = serde_json::from_slice(&decode_payload(&html, "d")).unwrap();
        let artifacts = data["artifacts"].as_array().expect("artifacts array");
        assert_eq!(artifacts.len(), 2);
        assert_eq!(artifacts[0]["path"], "out.txt");
        assert_eq!(artifacts[0]["size"], 12);
        assert_eq!(artifacts[0]["mime"], "text/plain");
        assert_eq!(artifacts[1]["path"], "sut/error.log");
        assert_eq!(artifacts[1]["size"], 4096);
        assert!(artifacts[1]["mime"].is_null());
    }

    #[test]
    fn closing_tag_in_payload_is_isolated_from_html() {
        // A test name carrying a literal `</script>` would, under the old
        // inline-JSON design, terminate the surrounding <script> tag. Under
        // the new design the payload sits inside a base64 string in a
        // `type="application/octet-stream"` tag, so `</script>` cannot
        // appear there by construction. Verify the literal round-trips and
        // does not leak unescaped into the HTML outside the payload tag.
        let mut log = sample_log("hostile");
        log.info.name = "evil</script>name".to_string();

        let html = render(&log).unwrap();

        let data: serde_json::Value = serde_json::from_slice(&decode_payload(&html, "d")).unwrap();
        assert_eq!(data["info"]["name"], "evil</script>name");

        // Outside the four payload tags, the only `</script>` allowed is
        // the bootstrap's closer. The payload tags contain pure base64,
        // which has no `<` characters at all.
        let opener = "<script type=\"application/octet-stream\" id=\"d\">";
        let start = html.find(opener).unwrap() + opener.len();
        let end = html[start..].find("</script>").unwrap() + start;
        let payload = &html[start..end];
        assert!(payload.bytes().all(|b| b != b'<'));
    }

    #[test]
    fn bootstrap_shows_browser_floor_when_decompression_stream_missing() {
        // Without a headless-JS runtime we can't *execute* the fallback path,
        // but the contract is small enough to lock in structurally. If any
        // of these checks fail the fallback is broken — a Safari 15 user
        // would see a blank page or a half-rendered failure instead of the
        // documented one-liner.
        let html = render(&sample_log("any")).unwrap();
        let bootstrap = bootstrap_script(&html);

        // (a) Guard exists.
        assert!(
            bootstrap.contains("!window.DecompressionStream"),
            "bootstrap is missing the `!window.DecompressionStream` guard",
        );

        // (b) The fallback message names every supported browser floor
        // documented in 04-ci-integration.md / 05-test-log-viewer.md.
        for marker in ["Chrome 80+", "Firefox 113+", "Safari 16.4+"] {
            assert!(
                bootstrap.contains(marker),
                "bootstrap fallback message is missing `{marker}`",
            );
        }

        // (c) The message is set via `textContent` (not `innerHTML`) so
        // any future change that interpolates user-controlled strings can't
        // accidentally inject markup. Locks in the safer pattern.
        assert!(
            bootstrap.contains("document.body.textContent"),
            "fallback assigns to something other than `document.body.textContent`",
        );

        // (d) The guard short-circuits before the unzip helpers are
        // invoked. We verify by ordering: the `return` that exits the
        // fallback branch comes before any `await unzip(`.
        let return_idx = bootstrap
            .find("return;")
            .expect("fallback branch is missing an early `return`");
        let first_unzip = bootstrap
            .find("await unzip(")
            .expect("bootstrap no longer calls unzip — refactor invalidates this test");
        assert!(
            return_idx < first_unzip,
            "the fallback `return` no longer precedes the first `unzip` call \
             — DecompressionStream-less browsers may try to decompress anyway",
        );
    }

    /// Slice out the bootstrap `<script>` body (the bare `<script>...</script>`
    /// pair following the four payload tags). The payload tags all use
    /// `type="application/octet-stream"`, so the bootstrap is the first
    /// `<script>` opener with no `type=` attribute.
    fn bootstrap_script(html: &str) -> &str {
        let opener = "<script>";
        let start = html.find(opener).expect("bootstrap opener missing") + opener.len();
        let end = html[start..]
            .find("</script>")
            .expect("bootstrap closer missing")
            + start;
        &html[start..end]
    }

    #[test]
    fn write_creates_event_html_under_log_dir() {
        let dir = std::env::temp_dir().join(format!(
            "relux-event-html-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let result = write(&dir, &sample_log("disk")).map(|()| dir.join("event.html"));

        // Always clean up, even if the assertion below fails.
        let path = match result {
            Ok(p) => p,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&dir);
                panic!("write failed: {e}");
            }
        };

        let html = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(html.starts_with("<!doctype html>"));
        let data: serde_json::Value = serde_json::from_slice(&decode_payload(&html, "d")).unwrap();
        assert_eq!(data["info"]["name"], "disk");
    }
}
