//! Per-run `index.html` generator. Lists every test in a run with its
//! outcome, duration, progress string, and (when available) a link to the
//! per-test artifact directory. Self-contained — no event-log dependencies.

use std::fmt::Write as FmtWrite;
use std::fs;
use std::path::Path;

use crate::report::result::Outcome;
use crate::report::result::TestResult;
use crate::report::result::format_duration;

const CSS: &str = r#"
:root {
  --bg: #fff; --fg: #222; --muted: #888; --row-alt: #f6f6f6;
  --tbl-border: #ddd; --link: #1a6dcc;
  --pass: #1a8a3f; --fail: #cc2222; --invalid: #cc6622;
}
@media (prefers-color-scheme: dark) {
  :root {
    --bg: #1a1a2e; --fg: #d4d4d4; --muted: #777; --row-alt: #1e1e32;
    --tbl-border: #333; --link: #5cadff;
    --pass: #4dd87a; --fail: #ff5555; --invalid: #f0a060;
  }
}
* { margin: 0; padding: 0; box-sizing: border-box; }
body {
  font-family: ui-monospace, "Cascadia Code", "Fira Code", Menlo, Consolas, monospace;
  font-size: 13px; line-height: 1.5;
  background: var(--bg); color: var(--fg);
  margin: 0 auto; padding: 16px;
}
h1 { font-size: 1.3em; margin-bottom: 8px; text-align: center; }
p { text-align: center; }
a { color: var(--link); text-decoration: none; }
a:hover { text-decoration: underline; }
table.summary { border-collapse: collapse; max-width: 960px; margin: 8px auto; }
table.summary th, table.summary td {
  border: 1px solid var(--tbl-border); padding: 4px 8px; text-align: left;
}
table.summary tr:nth-child(even) { background: var(--row-alt); }
.pass { color: var(--pass); }
.fail { color: var(--fail); }
.skip { color: var(--muted); }
.invalid { color: var(--invalid); }
"#;

const FOOTER: &str = "</body></html>\n";

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn header(title: &str) -> String {
    format!(
        "<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\">\
         <title>{}</title><style>{CSS}</style></head><body>\n",
        html_escape(title)
    )
}

pub fn generate(run_dir: &Path, results: &[TestResult]) {
    let mut html = header("relux run summary");
    let run_name = run_dir.file_name().unwrap_or_default().to_string_lossy();
    let _ = writeln!(html, "<h1>Run: {}</h1>", html_escape(&run_name));

    let passed = results
        .iter()
        .filter(|r| matches!(r.outcome, Outcome::Pass))
        .count();
    let failed = results
        .iter()
        .filter(|r| matches!(r.outcome, Outcome::Fail(_)))
        .count();
    let skipped = results
        .iter()
        .filter(|r| matches!(r.outcome, Outcome::Skipped(_)))
        .count();
    let invalid = results
        .iter()
        .filter(|r| matches!(r.outcome, Outcome::Invalid(_)))
        .count();
    let _ = writeln!(
        html,
        "<p>{passed} passed, {failed} failed, {invalid} invalid, {skipped} skipped</p>"
    );

    let mut report_links = Vec::new();
    if run_dir.join("results.tap").exists() {
        report_links.push("<a href=\"results.tap\">TAP</a>");
    }
    if run_dir.join("junit.xml").exists() {
        report_links.push("<a href=\"junit.xml\">JUnit XML</a>");
    }
    if !report_links.is_empty() {
        let _ = writeln!(html, "<p>Reports: {}</p>", report_links.join(" &middot; "));
    }

    let _ = writeln!(html, "<table class=\"summary\">");
    let _ = writeln!(
        html,
        "<tr><th>Test</th><th>Result</th><th>Duration</th><th>Progress</th></tr>"
    );
    for result in results {
        let (class, label) = match &result.outcome {
            Outcome::Pass => ("pass", "PASS".to_string()),
            Outcome::Fail(_) => ("fail", "FAIL".to_string()),
            Outcome::Skipped(r) => ("skip", format!("SKIP: {r}")),
            Outcome::Invalid(r) => ("invalid", format!("INVALID: {r}")),
        };
        let link = if let Some(log_dir) = &result.log_dir {
            let rel = log_dir.strip_prefix(run_dir).unwrap_or(log_dir);
            format!(
                "<a href=\"{}/event.html\">{}</a>",
                rel.display(),
                html_escape(&result.test_name)
            )
        } else {
            html_escape(&result.test_name)
        };
        let _ = writeln!(
            html,
            "<tr><td>{link}</td><td class=\"{class}\">{label}</td>\
             <td>{}</td><td>{}</td></tr>",
            format_duration(result.duration),
            html_escape(&result.progress)
        );
    }
    let _ = writeln!(html, "</table>");
    html.push_str(FOOTER);

    let path = run_dir.join("index.html");
    let _ = fs::write(path, html);
}
