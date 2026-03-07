use std::collections::HashMap;
use std::fmt::Write as FmtWrite;
use std::fs;
use std::path::Path;
use std::time::Duration;

use crate::runtime::event_log::{LogEvent, LogEventKind};
use crate::runtime::result::{Outcome, TestResult};

const CSS: &str = r#"
:root{--bg:#fff;--fg:#222;--muted:#888;--ts-fg:#999;--send:#1a6dcc;--recv:#1a8a3f;
--match:#7e3fba;--err:#cc2222;--row-alt:#f6f6f6;--highlight:#fff3cd;--hl-border:#e0a800;
--tbl-border:#ddd;--link:#1a6dcc}
@media(prefers-color-scheme:dark){:root{--bg:#1a1a2e;--fg:#d4d4d4;--muted:#777;
--ts-fg:#666;--send:#5cadff;--recv:#4dd87a;--match:#b87fff;--err:#ff5555;
--row-alt:#1e1e32;--highlight:#3a3520;--hl-border:#b8860b;--tbl-border:#333;--link:#5cadff}}
*{margin:0;padding:0;box-sizing:border-box}
body{font-family:ui-monospace,"Cascadia Code","Fira Code",Menlo,Consolas,monospace;
font-size:13px;line-height:1.5;background:var(--bg);color:var(--fg);max-width:960px;
margin:0 auto;padding:16px}
h1{font-size:1.3em;margin-bottom:8px}
h2{font-size:1.1em;margin:16px 0 6px}
a{color:var(--link);text-decoration:none}
a:hover{text-decoration:underline}
table.log{width:100%;border-collapse:collapse;border:none}
table.log td{padding:1px 6px;vertical-align:top;white-space:pre-wrap;word-break:break-all}
table.log td.ts{width:10ch;color:var(--ts-fg)}
table.log td.ts a{color:var(--ts-fg);text-decoration:underline}
table.log td.ts a:hover{text-decoration:underline}
table.log td.sh{width:20ch;color:var(--muted)}
table.log td.kind{width:10ch;font-weight:600}
table.log tr:target{background:var(--highlight);outline:2px solid var(--hl-border);border-radius:3px}
table.summary{border-collapse:collapse;width:100%;margin:8px 0}
table.summary th,table.summary td{border:1px solid var(--tbl-border);padding:4px 8px;text-align:left}
table.summary tr:nth-child(even){background:var(--row-alt)}
.pass{color:var(--recv)}.fail{color:var(--err)}.skip{color:var(--muted)}
.send{color:var(--send)}.recv{color:var(--recv)}.match-ev{color:var(--match)}.err{color:var(--err)}
details{margin:4px 0}summary{cursor:pointer;color:var(--muted)}
.hdr{margin-bottom:12px;padding-bottom:8px;border-bottom:1px solid var(--tbl-border)}
.hdr a{margin-right:12px}
"#;

fn fmt_duration(d: &Duration) -> String {
    let secs = d.as_secs();
    let millis = d.subsec_millis();
    format!("+{secs}.{millis:03}s")
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn event_type_class(kind: &LogEventKind) -> (&str, &str) {
    match kind {
        LogEventKind::Send { .. } => ("send", "send"),
        LogEventKind::Recv { .. } => ("recv", "recv"),
        LogEventKind::MatchStart { .. } | LogEventKind::MatchDone { .. } => ("match", "match-ev"),
        LogEventKind::NegMatchStart { .. } | LogEventKind::NegMatchPass { .. } => ("neg-match", "match-ev"),
        LogEventKind::NegMatchFail { .. } => ("neg-match", "err"),
        LogEventKind::Timeout { .. } => ("timeout", "err"),
        LogEventKind::FailPatternSet { .. } => ("fail-pat", "err"),
        LogEventKind::FailPatternTriggered { .. } => ("FAIL", "err"),
        LogEventKind::EffectSetup { .. } => ("effect+", ""),
        LogEventKind::EffectTeardown { .. } => ("effect-", ""),
        LogEventKind::Sleep { .. } => ("sleep", ""),
        LogEventKind::Annotate { .. } => ("note", ""),
        LogEventKind::Log { .. } => ("log", ""),
        LogEventKind::VarLet { .. } => ("let", ""),
        LogEventKind::VarAssign { .. } => ("assign", ""),
        LogEventKind::FnEnter { .. } => ("fn {", ""),
        LogEventKind::FnExit => ("fn }", ""),
        LogEventKind::Cleanup { .. } => ("cleanup", ""),
        LogEventKind::ShellSwitch { .. } => ("shell", ""),
    }
}

fn event_data(kind: &LogEventKind) -> String {
    match kind {
        LogEventKind::Send { data } => html_escape(data),
        LogEventKind::Recv { data } => html_escape(data),
        LogEventKind::MatchStart { pattern, is_regex } => {
            let prefix = if *is_regex { "regex " } else { "" };
            format!("{prefix}{}", html_escape(pattern))
        }
        LogEventKind::MatchDone { matched, elapsed } => {
            format!("{} ({})", html_escape(matched), fmt_duration(elapsed))
        }
        LogEventKind::NegMatchStart { pattern, is_regex } => {
            let prefix = if *is_regex { "regex " } else { "" };
            format!("!{prefix}{}", html_escape(pattern))
        }
        LogEventKind::NegMatchPass { pattern, elapsed } => {
            format!("!{} (pass, {})", html_escape(pattern), fmt_duration(elapsed))
        }
        LogEventKind::NegMatchFail { pattern, matched_text } => {
            format!("!{} found: {}", html_escape(pattern), html_escape(matched_text))
        }
        LogEventKind::Timeout { pattern } => html_escape(pattern),
        LogEventKind::FailPatternSet { pattern } => html_escape(pattern),
        LogEventKind::FailPatternTriggered { pattern, matched_line } => {
            format!("{} matched: {}", html_escape(pattern), html_escape(matched_line))
        }
        LogEventKind::EffectSetup { effect } => html_escape(effect),
        LogEventKind::EffectTeardown { effect } => html_escape(effect),
        LogEventKind::Sleep { duration } => format!("{duration:?}"),
        LogEventKind::Annotate { text } => html_escape(text),
        LogEventKind::Log { message } => html_escape(message),
        LogEventKind::VarLet { name, value } => {
            format!("{} = {}", html_escape(name), html_escape(value))
        }
        LogEventKind::VarAssign { name, value } => {
            format!("{} = {}", html_escape(name), html_escape(value))
        }
        LogEventKind::FnEnter { name } => html_escape(name),
        LogEventKind::FnExit => String::new(),
        LogEventKind::Cleanup { shell } => html_escape(shell),
        LogEventKind::ShellSwitch { name } => html_escape(name),
    }
}

fn html_header(title: &str, extra_head: &str) -> String {
    format!(
        "<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\">\
         <title>{}</title><style>{CSS}</style>{extra_head}</head><body>\n",
        html_escape(title)
    )
}

const HTML_FOOTER: &str = "</body></html>\n";

// ─── Run summary (index.html) ──────────────────────────────

pub fn generate_run_summary(run_dir: &Path, results: &[TestResult]) {
    let mut html = html_header("relux run summary", "");
    let run_name = run_dir
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let _ = writeln!(html, "<h1>Run: {}</h1>", html_escape(&run_name));

    let passed = results.iter().filter(|r| matches!(r.outcome, Outcome::Pass)).count();
    let failed = results.iter().filter(|r| matches!(r.outcome, Outcome::Fail(_))).count();
    let skipped = results.iter().filter(|r| matches!(r.outcome, Outcome::Skipped(_))).count();
    let _ = writeln!(
        html,
        "<p>{passed} passed, {failed} failed, {skipped} skipped</p>"
    );

    let _ = writeln!(html, "<table class=\"summary\">");
    let _ = writeln!(html, "<tr><th>Test</th><th>Result</th><th>Duration</th><th>Progress</th></tr>");
    for result in results {
        let (class, label) = match &result.outcome {
            Outcome::Pass => ("pass", "PASS".to_string()),
            Outcome::Fail(_) => ("fail", "FAIL".to_string()),
            Outcome::Skipped(r) => ("skip", format!("SKIP: {r}")),
        };
        let link = if let Some(log_dir) = &result.log_dir {
            let rel = log_dir
                .strip_prefix(run_dir)
                .unwrap_or(log_dir);
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
             <td>{:?}</td><td>{}</td></tr>",
            result.duration,
            html_escape(&result.progress)
        );
    }
    let _ = writeln!(html, "</table>");
    html.push_str(HTML_FOOTER);

    let path = run_dir.join("index.html");
    let _ = fs::write(path, html);
}

// ─── Per-test HTML logs ─────────────────────────────────────

pub fn generate_html_logs(
    log_dir: &Path,
    test_name: &str,
    events: &[LogEvent],
    _run_dir: &Path,
) {
    let shells = collect_shells(events);
    let shell_event_indices = build_shell_event_indices(events, &shells);

    generate_test_event_log(log_dir, test_name, events, &shells, &shell_event_indices);

    for shell in &shells {
        generate_shell_log(log_dir, shell, events, &shell_event_indices);
    }
}

fn collect_shells(events: &[LogEvent]) -> Vec<String> {
    let mut seen = HashMap::new();
    let mut order = Vec::new();
    for e in events {
        if !e.shell.is_empty() && !seen.contains_key(&e.shell) {
            seen.insert(e.shell.clone(), order.len());
            order.push(e.shell.clone());
        }
    }
    order
}

/// For each event, compute the per-shell event counter.
/// Returns a Vec parallel to `events` with (shell_event_index).
/// Also populates a map of shell -> next counter.
fn build_shell_event_indices(
    events: &[LogEvent],
    shells: &[String],
) -> Vec<usize> {
    let mut counters: HashMap<&str, usize> = HashMap::new();
    for s in shells {
        counters.insert(s.as_str(), 0);
    }
    events
        .iter()
        .map(|e| {
            if e.shell.is_empty() {
                0
            } else {
                let c = counters.entry(&e.shell).or_insert(0);
                let idx = *c;
                *c += 1;
                idx
            }
        })
        .collect()
}

fn generate_test_event_log(
    log_dir: &Path,
    test_name: &str,
    events: &[LogEvent],
    _shells: &[String],
    shell_event_indices: &[usize],
) {
    let mut html = html_header(&format!("test: {test_name}"), "");
    let _ = writeln!(html, "<h1>Test: {}</h1>", html_escape(test_name));
    let _ = writeln!(html, "<table class=\"log\">");

    for (i, event) in events.iter().enumerate() {
        let shell_idx = shell_event_indices[i];
        let anchor = if event.shell.is_empty() {
            format!("e{i}")
        } else {
            format!("{}-e{shell_idx}", event.shell)
        };
        let (type_label, type_class) = event_type_class(&event.kind);
        let data = event_data(&event.kind);

        let ts_str = fmt_duration(&event.timestamp);
        let ts_cell = if !event.shell.is_empty() {
            let shell_file = format!("{}.html", event.shell);
            format!(
                "<td class=\"ts\"><a href=\"{}#e{shell_idx}\">{ts_str}</a></td>",
                html_escape(&shell_file)
            )
        } else {
            format!("<td class=\"ts\">{ts_str}</td>")
        };

        let shell_cell = format!("<td class=\"sh\">{}</td>", html_escape(&event.shell));
        let class_attr = if type_class.is_empty() {
            String::new()
        } else {
            format!(" class=\"{type_class}\"")
        };

        let _ = writeln!(
            html,
            "<tr id=\"{anchor}\">{ts_cell}{shell_cell}\
             <td class=\"kind\"{class_attr}>{type_label}</td>\
             <td{class_attr}>{data}</td></tr>"
        );
    }

    let _ = writeln!(html, "</table>");
    html.push_str(HTML_FOOTER);
    let _ = fs::write(log_dir.join("event.html"), html);
}

fn generate_shell_log(
    log_dir: &Path,
    shell: &str,
    events: &[LogEvent],
    shell_event_indices: &[usize],
) {
    let mut html = html_header(&format!("shell: {shell}"), "");
    let _ = writeln!(html, "<h1>Shell: {}</h1>", html_escape(shell));

    let _ = writeln!(html, "<div class=\"hdr\">");
    for ext in &["stdin.raw", "stdin.log", "stdout.raw", "stdout.log"] {
        let _ = write!(
            html,
            "<a href=\"{}.{ext}\">{ext}</a>",
            html_escape(shell)
        );
    }
    let _ = writeln!(html, "</div>");

    let _ = writeln!(html, "<table class=\"log\">");

    for (i, event) in events.iter().enumerate() {
        if event.shell != shell {
            continue;
        }
        let shell_idx = shell_event_indices[i];
        let (type_label, type_class) = event_type_class(&event.kind);
        let data = event_data(&event.kind);

        let ts_str = fmt_duration(&event.timestamp);
        let test_anchor = format!("{shell}-e{shell_idx}");
        let ts_cell = format!(
            "<td class=\"ts\"><a href=\"event.html#{test_anchor}\">{ts_str}</a></td>"
        );

        let class_attr = if type_class.is_empty() {
            String::new()
        } else {
            format!(" class=\"{type_class}\"")
        };

        let _ = writeln!(
            html,
            "<tr id=\"e{shell_idx}\">{ts_cell}\
             <td class=\"kind\"{class_attr}>{type_label}</td>\
             <td{class_attr}>{data}</td></tr>"
        );
    }

    let _ = writeln!(html, "</table>");
    html.push_str(HTML_FOOTER);
    let _ = fs::write(log_dir.join(format!("{shell}.html")), html);
}
