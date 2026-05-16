//! Per-test `event.html` emitter.
//!
//! Writes a single self-contained HTML file beside `events.json`. The
//! `StructuredLog` payload is inlined as `window.RELUX_DATA`; the
//! highlight.js core + Relux language definition are inlined as a
//! second / third `<script>` so the source pane can render syntax-
//! highlighted code without a separate served asset; finally the
//! gzipped Svelte bundle (`relux_runtime::viewer::bundle_gz()`) is
//! decompressed into a fourth `<script>` tag — no `fetch`, no CORS,
//! opens directly via `file://`.

use std::io::Read;
use std::path::Path;

use flate2::read::GzDecoder;

use crate::observe::structured::StructuredLog;
use crate::report::hljs_init::HLJS_RELUX_INIT;
use crate::viewer;

const HEADER: &str = "<!doctype html>\n\
    <html lang=\"en\">\n\
    <head><meta charset=\"utf-8\"><title>Relux test report</title>\
    <style>html,body{margin:0;padding:0}</style></head>\n\
    <body><div id=\"app\"></div>\n\
    <script>window.RELUX_DATA = ";
const SCRIPT_BREAK: &str = ";</script>\n<script>";
const SCRIPT_GAP: &str = "</script>\n<script>";
const FOOTER: &str = "</script>\n</body></html>\n";

pub fn write(log_dir: &Path, structured: &StructuredLog) -> std::io::Result<()> {
    let html = render(structured)?;
    std::fs::write(log_dir.join("event.html"), html)
}

fn render(structured: &StructuredLog) -> std::io::Result<String> {
    let mut json = serde_json::to_string(structured).map_err(std::io::Error::other)?;
    // Defuse `</` so a Recv/Annotate/etc. payload cannot terminate the
    // surrounding <script> tag. Standard JSON-in-HTML-script escape.
    if json.contains("</") {
        json = json.replace("</", "<\\/");
    }

    let mut hljs = String::new();
    GzDecoder::new(viewer::hljs_gz()).read_to_string(&mut hljs)?;

    let mut bundle = String::new();
    GzDecoder::new(viewer::bundle_gz()).read_to_string(&mut bundle)?;

    let mut html = String::with_capacity(
        HEADER.len()
            + json.len()
            + SCRIPT_BREAK.len()
            + hljs.len()
            + SCRIPT_GAP.len()
            + HLJS_RELUX_INIT.len()
            + SCRIPT_GAP.len()
            + bundle.len()
            + FOOTER.len(),
    );
    html.push_str(HEADER);
    html.push_str(&json);
    html.push_str(SCRIPT_BREAK);
    html.push_str(&hljs);
    html.push_str(SCRIPT_GAP);
    html.push_str(HLJS_RELUX_INIT);
    html.push_str(SCRIPT_GAP);
    html.push_str(&bundle);
    html.push_str(FOOTER);
    Ok(html)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::observe::structured::EnvInfo;
    use crate::observe::structured::StructuredLog;
    use crate::observe::structured::TestInfo;
    use crate::observe::structured::TestOutcome;

    fn sample_log(test_name: &str) -> StructuredLog {
        StructuredLog {
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

    #[test]
    fn html_inlines_payload_and_bundle_entry_hook() {
        let html = render(&sample_log("hello-world")).unwrap();
        assert!(html.contains("window.RELUX_DATA = "));
        assert!(html.contains("\"name\":\"hello-world\""));
        // The Svelte bundle reads `window.RELUX_DATA` at mount time; if this
        // string disappears, decompression silently dropped the bundle body.
        assert!(html.contains("RELUX_DATA"));
    }

    #[test]
    fn render_inlines_artifacts_into_window_relux_data() {
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
        assert!(html.contains("\"artifacts\":["), "artifacts array missing");
        assert!(html.contains("\"path\":\"out.txt\""));
        assert!(html.contains("\"path\":\"sut/error.log\""));
        assert!(html.contains("\"size\":4096"));
        assert!(html.contains("\"mime\":\"text/plain\""));
        assert!(html.contains("\"mime\":null") || html.contains("\"mime\": null"));
    }

    #[test]
    fn closing_tag_in_payload_is_escaped() {
        let mut log = sample_log("hostile");
        // A test name carrying a literal `</script>` would otherwise
        // terminate the surrounding <script> tag and break the page.
        log.info.name = "evil</script>name".to_string();

        let html = render(&log).unwrap();

        // The exact byte sequence `</script>` must not appear inside the
        // RELUX_DATA assignment — only `<\/script>` is allowed.
        let payload_end = html
            .find(";</script>\n<script>")
            .expect("SCRIPT_BREAK separator");
        let payload = &html[..payload_end];
        assert!(
            !payload.contains("</script>"),
            "unescaped </script> leaked into RELUX_DATA payload"
        );
        assert!(payload.contains("evil<\\/script>name"));
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
        assert!(html.contains("\"name\":\"disk\""));
    }
}
