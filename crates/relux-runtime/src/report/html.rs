use std::collections::HashMap;
use std::fmt::Write as FmtWrite;
use std::fs;
use std::path::Path;
use std::time::Duration;

use relux_core::table::SourceTable;

use crate::observe::event_log::BufferSnapshot;
use crate::observe::event_log::LogEvent;
use crate::observe::event_log::LogEventKind;
use crate::report::result::Outcome;
use crate::report::result::TestResult;
use crate::report::result::format_duration;

const CSS: &str = r#"
:root{--bg:#fff;--fg:#222;--muted:#888;--ts-fg:#999;--send:#1a6dcc;--recv:#1a8a3f;
--match:#7e3fba;--err:#cc2222;--log:#d97706;--row-alt:#f6f6f6;--highlight:#fff3cd;--hl-border:#e0a800;
--tbl-border:#ddd;--link:#1a6dcc}
@media(prefers-color-scheme:dark){:root{--bg:#1a1a2e;--fg:#d4d4d4;--muted:#777;
--ts-fg:#666;--send:#5cadff;--recv:#4dd87a;--match:#b87fff;--err:#ff5555;--log:#f59e0b;
--row-alt:#1e1e32;--highlight:#3a3520;--hl-border:#b8860b;--tbl-border:#333;--link:#5cadff}}
*{margin:0;padding:0;box-sizing:border-box}
body{font-family:ui-monospace,"Cascadia Code","Fira Code",Menlo,Consolas,monospace;
font-size:13px;line-height:1.5;background:var(--bg);color:var(--fg);
margin:0 auto;padding:16px}
h1{font-size:1.3em;margin-bottom:8px;text-align:center}
p{text-align:center}
h2{font-size:1.1em;margin:16px 0 6px}
a{color:var(--link);text-decoration:none}
a:hover{text-decoration:underline}
table.log{border-collapse:collapse;border:none;margin:0 auto}
table.log td{padding:1px 6px;vertical-align:top;white-space:pre-wrap;word-break:break-all}
table.log td.ts{white-space:nowrap;color:var(--ts-fg)}
table.log td.ts a{color:var(--ts-fg);text-decoration:underline}
table.log td.ts a:hover{text-decoration:underline}
table.log td.sh{white-space:nowrap;color:var(--muted)}
table.log td.kind{white-space:nowrap;font-weight:600}
table.log tr:nth-child(even){background:var(--row-alt)}
table.log tr:target{background:var(--highlight);outline:2px solid var(--hl-border);border-radius:3px}
table.summary{border-collapse:collapse;max-width:960px;margin:8px auto}
table.summary th,table.summary td{border:1px solid var(--tbl-border);padding:4px 8px;text-align:left}
table.summary tr:nth-child(even){background:var(--row-alt)}
.pass{color:var(--recv)}.fail{color:var(--err)}.skip{color:var(--muted)}
.send{color:var(--send)}.recv{color:var(--recv)}.match-ev{color:var(--match)}.err{color:var(--err)}.log-ev{color:var(--log);font-weight:600}
details{margin:4px 0}summary{cursor:pointer;color:var(--muted)}
.hdr{margin-bottom:12px;padding-bottom:8px;border-bottom:1px solid var(--tbl-border);text-align:center}
.hdr a{margin-right:12px}
table.log td.data{width:80ch;min-width:80ch;max-width:80ch;overflow:hidden;text-overflow:ellipsis}
table.log td.buf{width:80ch;min-width:80ch;max-width:80ch;white-space:pre-wrap;word-break:break-all}
.buf-box{padding:2px 6px;border:1px solid var(--tbl-border);border-radius:3px;display:block;
width:100%;min-height:100%;box-sizing:border-box}
.buf-skip{color:var(--muted)}.buf-match{color:var(--recv)}
table.log td.loc{white-space:nowrap;color:var(--muted)}
table.log td.loc a{color:var(--muted);text-decoration:underline}
"#;

fn fmt_duration(d: &Duration) -> String {
    format!("+{}", format_duration(*d))
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn event_type_class(kind: &LogEventKind) -> (&str, &str) {
    match kind {
        LogEventKind::Send { .. } => ("shell send", "send"),
        LogEventKind::Recv { .. } => ("shell recv", "recv"),
        LogEventKind::MatchStart { is_regex: true, .. } => ("regex match start", "match-ev"),
        LogEventKind::MatchStart {
            is_regex: false, ..
        } => ("literal match start", "match-ev"),
        LogEventKind::MatchDone { .. } => ("match done", "match-ev"),
        LogEventKind::Timeout { .. } => ("match timeout", "err"),
        LogEventKind::BufferReset { .. } => ("buffer reset", "err"),
        LogEventKind::FailPatternSet { .. } => ("fail set", "err"),
        LogEventKind::FailPatternCleared => ("fail clear", ""),
        LogEventKind::FailPatternTriggered { .. } => ("fail trigger", "err"),
        LogEventKind::EffectSetup { .. } => ("effect setup", ""),
        LogEventKind::EffectTeardown { .. } => ("effect teardown", ""),
        LogEventKind::SleepStart { .. } => ("sleep start", ""),
        LogEventKind::SleepDone => ("sleep done", ""),
        LogEventKind::Annotate { .. } => ("annotate", ""),
        LogEventKind::Log { .. } => ("log", "log-ev"),
        LogEventKind::VarLet { .. } => ("var let", ""),
        LogEventKind::VarAssign { .. } => ("var assign", ""),
        LogEventKind::FnEnter { .. } => ("fn enter", ""),
        LogEventKind::FnExit { .. } => ("fn exit", ""),
        LogEventKind::Cleanup { .. } => ("cleanup", ""),
        LogEventKind::ShellSwitch { .. } => ("shell switch", ""),
        LogEventKind::ShellSpawn { .. } => ("shell spawn", ""),
        LogEventKind::ShellReady { .. } => ("shell ready", ""),
        LogEventKind::ShellTerminate { .. } => ("shell exit", ""),
        LogEventKind::ShellAlias { .. } => ("shell alias", ""),
        LogEventKind::TimeoutSet { .. } => ("timeout set", ""),
        LogEventKind::StringEval { .. } => ("string eval", ""),
        LogEventKind::Interpolation { .. } => ("string interp", ""),
        LogEventKind::Failure => ("failure", "err"),
        LogEventKind::Error { .. } => ("error", "err"),
        LogEventKind::Warning { .. } => ("warning", "err"),
    }
}

fn render_kv(pairs: &[(String, String)]) -> String {
    if pairs.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for (k, v) in pairs {
        let _ = write!(
            out,
            "<br>&nbsp;&nbsp;{} = {}",
            html_escape(k),
            html_escape(v)
        );
    }
    out
}

fn render_value(label: &str, value: &str) -> String {
    let display = if value.is_empty() {
        "(empty string)"
    } else {
        value
    };
    format!("<br>&nbsp;&nbsp;{} = {}", label, html_escape(display))
}

fn event_data(kind: &LogEventKind) -> String {
    match kind {
        LogEventKind::Send { data } => html_escape(data),
        LogEventKind::Recv { data } => html_escape(data),
        LogEventKind::MatchStart { pattern, is_regex } => {
            let prefix = if *is_regex { "regex " } else { "" };
            format!("{prefix}{}", html_escape(pattern))
        }
        LogEventKind::MatchDone {
            matched,
            elapsed,
            captures,
            ..
        } => {
            let mut out = format!("{} ({})", html_escape(matched), fmt_duration(elapsed));
            if let Some(groups) = captures {
                let mut sorted: Vec<_> = groups.iter().collect();
                sorted.sort_by_key(|(k, _)| k.parse::<usize>().unwrap_or(usize::MAX));
                let pairs: Vec<(String, String)> = sorted
                    .iter()
                    .map(|(k, v)| (format!("${k}"), v.to_string()))
                    .collect();
                out.push_str(&render_kv(&pairs));
            }
            out
        }
        LogEventKind::BufferReset { .. } => String::new(),
        LogEventKind::Timeout { pattern, .. } => html_escape(pattern),
        LogEventKind::FailPatternSet { pattern } => html_escape(pattern),
        LogEventKind::FailPatternCleared => "(cleared)".to_string(),
        LogEventKind::FailPatternTriggered {
            pattern,
            matched_line,
            ..
        } => {
            format!(
                "{} matched: {}",
                html_escape(pattern),
                html_escape(matched_line)
            )
        }
        LogEventKind::EffectSetup { effect } => html_escape(effect),
        LogEventKind::EffectTeardown { effect } => html_escape(effect),
        LogEventKind::SleepStart { duration } => format!("{duration:?}"),
        LogEventKind::SleepDone => String::new(),
        LogEventKind::Annotate { text } => html_escape(text),
        LogEventKind::Log { message } => html_escape(message),
        LogEventKind::VarLet { name, value } => {
            format!("{} = {}", html_escape(name), html_escape(value))
        }
        LogEventKind::VarAssign { name, value } => {
            format!("{} = {}", html_escape(name), html_escape(value))
        }
        LogEventKind::FnEnter { name, args } => {
            let mut out = html_escape(name);
            out.push_str(&render_kv(args));
            out
        }
        LogEventKind::FnExit {
            name,
            return_value,
            restored_timeout,
            restored_fail_pattern,
        } => {
            let mut out = html_escape(name);
            out.push_str(&render_value("return", return_value));
            if let Some(t) = restored_timeout {
                out.push_str(&render_value("restored timeout", t));
            }
            if let Some(fp) = restored_fail_pattern {
                out.push_str(&render_value("restored fail pattern", fp));
            }
            out
        }
        LogEventKind::Cleanup { shell } => html_escape(shell),
        LogEventKind::ShellSwitch { name } => html_escape(name),
        LogEventKind::ShellSpawn { name, command } => {
            let mut out = html_escape(name);
            out.push_str(&render_value("command", command));
            out
        }
        LogEventKind::ShellReady { name } => html_escape(name),
        LogEventKind::ShellTerminate { name } => html_escape(name),
        LogEventKind::ShellAlias { name, source } => {
            format!("{} &lt;- {}", html_escape(name), html_escape(source))
        }
        LogEventKind::TimeoutSet { timeout, previous } => {
            format!("{} (was {})", html_escape(timeout), html_escape(previous))
        }
        LogEventKind::StringEval { result } => html_escape(result),
        LogEventKind::Interpolation {
            template,
            result,
            bindings,
        } => {
            let mut out = format!("{} -&gt; {}", html_escape(template), html_escape(result));
            out.push_str(&render_kv(bindings));
            out
        }
        LogEventKind::Failure => String::new(),
        LogEventKind::Error { message } => html_escape(message),
        LogEventKind::Warning { message } => html_escape(message),
    }
}

fn event_buffer(kind: &LogEventKind) -> Option<&BufferSnapshot> {
    match kind {
        LogEventKind::MatchDone { buffer, .. } => Some(buffer),
        LogEventKind::Timeout { buffer, .. } => Some(buffer),
        LogEventKind::FailPatternTriggered { buffer, .. } => Some(buffer),
        LogEventKind::BufferReset { buffer } => Some(buffer),
        _ => None,
    }
}

fn render_buffer(kind: &LogEventKind) -> String {
    let Some(snapshot) = event_buffer(kind) else {
        return String::new();
    };
    let inner = match snapshot {
        BufferSnapshot::Match {
            before,
            matched,
            after,
        } => {
            let is_neg = matches!(kind, LogEventKind::FailPatternTriggered { .. });
            let match_class = if is_neg { "buf-skip" } else { "buf-match" };
            let before_class = if is_neg { "" } else { " class=\"buf-skip\"" };
            let mut buf = String::new();
            if !before.is_empty() {
                let _ = write!(buf, "<span{before_class}>{}</span>", html_escape(before));
            }
            if !matched.is_empty() {
                let _ = write!(
                    buf,
                    "<span class=\"{match_class}\">{}</span>",
                    html_escape(matched)
                );
            }
            if !after.is_empty() {
                buf.push_str(&html_escape(after));
            }
            buf
        }
        BufferSnapshot::Tail { content } => {
            if matches!(kind, LogEventKind::BufferReset { .. }) {
                format!("<span class=\"buf-skip\">{}</span>", html_escape(content))
            } else {
                html_escape(content)
            }
        }
    };
    if inner.is_empty() {
        return String::new();
    }
    format!("<span class=\"buf-box\">{inner}</span>")
}

fn render_loc_cell(event: &LogEvent, log_dir: &Path, run_dir: &Path) -> String {
    let Some(loc) = &event.location else {
        return "<td class=\"loc\"></td>".to_string();
    };
    // Compute relative path from log_dir to run_dir/source/{file}.html
    let depth = log_dir
        .strip_prefix(run_dir)
        .map(|p| p.components().count())
        .unwrap_or(0);
    let up = "../".repeat(depth);
    let href = format!("{up}source/{}.html#L{}", loc.file, loc.line);
    format!(
        "<td class=\"loc\"><a href=\"{}\">{}</a></td>",
        html_escape(&href),
        html_escape(&format!("{}:{}", loc.file, loc.line))
    )
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
    html.push_str(HTML_FOOTER);

    let path = run_dir.join("index.html");
    let _ = fs::write(path, html);
}

// ─── Per-test HTML logs ─────────────────────────────────────

pub fn generate_html_logs(log_dir: &Path, test_name: &str, events: &[LogEvent], _run_dir: &Path) {
    let shells = collect_shells(events);
    let shell_event_indices = build_shell_event_indices(events, &shells);

    generate_test_event_log(
        log_dir,
        test_name,
        events,
        &shells,
        &shell_event_indices,
        _run_dir,
    );

    for shell in &shells {
        generate_shell_log(log_dir, shell, events, &shell_event_indices, _run_dir);
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
fn build_shell_event_indices(events: &[LogEvent], shells: &[String]) -> Vec<usize> {
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
    run_dir: &Path,
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
        let kind_class = if type_class.is_empty() {
            "kind".to_string()
        } else {
            format!("kind {type_class}")
        };
        let data_class = if type_class.is_empty() {
            "data".to_string()
        } else {
            format!("data {type_class}")
        };

        let buf_html = render_buffer(&event.kind);
        let buf_cell = format!("<td class=\"buf\">{buf_html}</td>");
        let loc_cell = render_loc_cell(event, log_dir, run_dir);

        let _ = writeln!(
            html,
            "<tr id=\"{anchor}\">{ts_cell}{loc_cell}{shell_cell}\
             <td class=\"{kind_class}\">{type_label}</td>\
             <td class=\"{data_class}\">{data}</td>{buf_cell}</tr>"
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
    run_dir: &Path,
) {
    let mut html = html_header(&format!("shell: {shell}"), "");
    let _ = writeln!(html, "<h1>Shell: {}</h1>", html_escape(shell));

    let _ = writeln!(html, "<div class=\"hdr\">");
    for ext in &["stdin.raw", "stdin.log", "stdout.raw", "stdout.log"] {
        let _ = write!(html, "<a href=\"{}.{ext}\">{ext}</a>", html_escape(shell));
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
        let ts_cell =
            format!("<td class=\"ts\"><a href=\"event.html#{test_anchor}\">{ts_str}</a></td>");

        let kind_class = if type_class.is_empty() {
            "kind".to_string()
        } else {
            format!("kind {type_class}")
        };
        let data_class = if type_class.is_empty() {
            "data".to_string()
        } else {
            format!("data {type_class}")
        };

        let buf_html = render_buffer(&event.kind);
        let buf_cell = format!("<td class=\"buf\">{buf_html}</td>");
        let loc_cell = render_loc_cell(event, log_dir, run_dir);

        let _ = writeln!(
            html,
            "<tr id=\"e{shell_idx}\">{ts_cell}{loc_cell}\
             <td class=\"{kind_class}\">{type_label}</td>\
             <td class=\"{data_class}\">{data}</td>{buf_cell}</tr>"
        );
    }

    let _ = writeln!(html, "</table>");
    html.push_str(HTML_FOOTER);
    let _ = fs::write(log_dir.join(format!("{shell}.html")), html);
}

// ─── Source pages ──────────────────────────────────────────

const SOURCE_CSS: &str = r#"
:root{--bg:#fff;--fg:#222;--line-num:#999;--highlight:#fff3cd;--hl-border:#e0a800;
--tbl-border:#ddd;--link:#1a6dcc;
--kw:#1a6dcc;--str:#1a8a3f;--comment:#888;--type:#7e3fba;--num:#d97706;--subst:#cc2222;--meta:#697070;--op:#ab5656}
@media(prefers-color-scheme:dark){:root{--bg:#1a1a2e;--fg:#d4d4d4;--line-num:#555;
--highlight:#3a3520;--hl-border:#b8860b;--tbl-border:#333;--link:#5cadff;
--kw:#5cadff;--str:#4dd87a;--comment:#777;--type:#b87fff;--num:#f59e0b;--subst:#ff5555;--meta:#888;--op:#b87fff}}
*{margin:0;padding:0;box-sizing:border-box}
body{font-family:ui-monospace,"Cascadia Code","Fira Code",Menlo,Consolas,monospace;
font-size:13px;line-height:1.5;background:var(--bg);color:var(--fg);padding:16px}
h1{font-size:1.1em;margin-bottom:8px;text-align:center}
a{color:var(--link);text-decoration:none}
.copy-btn{background:none;border:1px solid var(--tbl-border);border-radius:3px;
color:var(--muted,#888);cursor:pointer;font-size:0.9em;padding:2px 8px;margin-left:8px;
font-family:inherit}
.copy-btn:hover{color:var(--fg)}
table.source{border-collapse:collapse;width:160ch;margin:0 auto}
table.source td{padding:0 8px;vertical-align:top}
table.source td.num{white-space:nowrap;text-align:right;color:var(--line-num);
user-select:none;-webkit-user-select:none;padding-right:12px;border-right:1px solid var(--tbl-border)}
table.source td.num a{color:var(--line-num);text-decoration:none}
table.source td.code{white-space:pre;padding-left:12px}
table.source tr:target{background:var(--highlight);outline:2px solid var(--hl-border);border-radius:3px}
pre{margin:0}code{font-family:inherit}
.hljs{background:transparent!important;padding:0!important}
.hljs-keyword{color:var(--kw);font-weight:700}
.hljs-string{color:var(--str)}
.hljs-comment{color:var(--comment)}
.hljs-type{color:var(--type)}
.hljs-number{color:var(--num)}
.hljs-subst{color:var(--subst)}
.hljs-meta{color:var(--meta)}
.hljs-built_in{color:var(--kw)}
.hljs-symbol,.hljs-template-variable,.hljs-variable{color:var(--op)}
.hljs-emphasis{font-style:italic}
.hljs-strong{font-weight:700}
"#;

const HLJS_CORE_GZ: &[u8] = include_bytes!("../../../../vendor/highlight-11.11.1.min.js.gz");
const HLJS_RELUX: &str = include_str!("../../../../docs/reference/highlight-relux.js");

fn gunzip(data: &[u8]) -> String {
    use flate2::read::GzDecoder;
    use std::io::Read;
    let mut decoder = GzDecoder::new(data);
    let mut out = String::new();
    decoder
        .read_to_string(&mut out)
        .expect("gzip decode failed");
    out
}

/// Copy source files to `run_dir/source/` and generate syntax-highlighted HTML pages.
pub fn generate_source_pages(run_dir: &Path, source_table: &SourceTable, project_root: &Path) {
    let source_dir = run_dir.join("source");
    let _ = fs::create_dir_all(&source_dir);

    let hljs_core = gunzip(HLJS_CORE_GZ);

    for (file_id, source_file) in source_table.as_vec() {
        let rel_path = source_file
            .path
            .strip_prefix(project_root)
            .unwrap_or(&source_file.path);

        // Copy raw source file
        let raw_dest = source_dir.join(rel_path);
        if let Some(parent) = raw_dest.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(&raw_dest, &source_file.source);

        // Generate HTML page
        let html_dest = source_dir.join(format!("{}.html", rel_path.display()));
        if let Some(parent) = html_dest.parent() {
            let _ = fs::create_dir_all(parent);
        }

        let rel_display = rel_path.display().to_string();
        let _ = file_id; // used only for iteration

        let mut html = String::new();
        let _ = write!(
            html,
            "<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\">\
             <title>{}</title>\
             <style>{SOURCE_CSS}</style>\
             <script>{hljs_core}</script>\
             <script>{HLJS_RELUX}</script>\
             </head><body>\n",
            html_escape(&rel_display)
        );

        let escaped = html_escape(&rel_display);
        let js_path = rel_display.replace('\\', "\\\\").replace('\'', "\\'");
        let _ = writeln!(
            html,
            "<h1>{escaped}<button class=\"copy-btn\" onclick=\"var l=location.hash.match(/^#L(\\d+)/);\
             navigator.clipboard.writeText('{js_path}'+(l?':'+l[1]:''))\
             .then(()=>{{this.textContent='copied'}}).catch(()=>{{}})\">copy path</button></h1>"
        );
        let _ = writeln!(html, "<table class=\"source\">");

        for (i, line) in source_file.source.lines().enumerate() {
            let n = i + 1;
            let _ = writeln!(
                html,
                "<tr id=\"L{n}\"><td class=\"num\"><a href=\"#L{n}\">{n}</a></td>\
                 <td class=\"code\"><code class=\"language-relux\">{}</code></td></tr>",
                html_escape(line)
            );
        }

        let _ = writeln!(html, "</table>");
        let _ = writeln!(
            html,
            "<script>document.querySelectorAll('code.language-relux').forEach(el => hljs.highlightElement(el));</script>"
        );
        html.push_str(HTML_FOOTER);
        let _ = fs::write(&html_dest, html);
    }
}
