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
  --bg: #0f1620;
  --bg-deep: #0a0f17;
  --paper: #18212e;
  --paper-2: #1d2735;
  --ink: #e8e4d6;
  --ink-dim: #9aa3ad;
  --ink-faint: #5b6470;
  --accent: #ffd166;
  --accent-2: #6fd3a3;
  --danger: #ff7a7a;
  --info: #7aa7d9;
  --pass: var(--accent-2);
  --fail: var(--danger);
  --cancel: var(--accent);
  --skip: var(--ink-faint);
  --invalid: var(--accent);
  --font-body: "Inter", -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
  --font-mono: "JetBrains Mono", ui-monospace, "Cascadia Code", "Fira Code", "Source Code Pro", Menlo, Consolas, monospace;
  --gap-xs: 4px;
  --gap-sm: 8px;
  --gap-md: 12px;
  --gap-lg: 18px;
  --gap-xl: 28px;
  --radius: 6px;
  --border: rgba(232, 228, 214, 0.18);
}
* { margin: 0; padding: 0; box-sizing: border-box; }
html {
  background: var(--bg);
}
body {
  background: var(--bg);
  color: var(--ink);
  font-family: var(--font-body);
  font-size: 15px;
  line-height: 1.5;
  max-width: 1100px;
  margin: 0 auto;
}
a { color: inherit; text-decoration: none; }

.appbar {
  display: flex;
  align-items: center;
  gap: var(--gap-md);
  padding: var(--gap-sm) var(--gap-lg);
  border-bottom: 1px solid var(--border);
  background: var(--bg);
  font-size: 0.9rem;
  color: var(--ink-dim);
}
.appbar .crumbs {
  color: var(--ink);
  font-family: var(--font-mono);
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.appbar .crumbs .sep {
  color: var(--ink-faint);
  margin: 0 6px;
}
.appbar .crumbs .run-id {
  color: var(--ink-faint);
}
.appbar .pill {
  font-family: var(--font-mono);
  font-size: 0.72rem;
  letter-spacing: 0.05em;
  text-transform: uppercase;
  padding: 2px 10px;
  border: 1px solid currentColor;
  border-radius: 100px;
  flex: 0 0 auto;
}
.appbar .pill.pass { color: var(--pass); }
.appbar .pill.fail { color: var(--fail); }
.appbar .pill.cancel { color: var(--cancel); }
.appbar .timing {
  margin-left: auto;
  font-family: var(--font-mono);
  font-size: 0.78rem;
  color: var(--ink-dim);
  flex: 0 0 auto;
}
.appbar .timing .cum {
  color: var(--ink-faint);
}

.reports-bar {
  padding: 6px var(--gap-lg);
  border-bottom: 1px dashed var(--border);
  font-family: var(--font-mono);
  font-size: 0.76rem;
  color: var(--ink-dim);
}
.reports-bar a { color: var(--ink-dim); }
.reports-bar a:hover { color: var(--ink); }
.reports-bar .sep { color: var(--ink-faint); margin: 0 6px; }

.search-row {
  padding: var(--gap-sm) var(--gap-lg);
  border-bottom: 1px dashed var(--border);
}
.search-input {
  display: flex;
  align-items: center;
  gap: var(--gap-sm);
  padding: 6px 10px;
  border: 1px solid var(--accent);
  border-radius: var(--radius);
  background: color-mix(in srgb, var(--accent) 4%, transparent);
}
.search-input input {
  flex: 1 1 auto;
  background: transparent;
  border: none;
  color: var(--ink);
  font: inherit;
  font-family: var(--font-mono);
  font-size: 0.85rem;
  outline: none;
}
.search-input .glyph { color: var(--ink-faint); }
.search-input .count {
  font-family: var(--font-mono);
  color: var(--ink-faint);
  font-size: 0.72rem;
}
.search-input .kbd {
  font-family: var(--font-mono);
  font-size: 0.6rem;
  font-weight: 600;
  line-height: 1;
  padding: 2px 4px;
  border: 1px solid var(--accent);
  border-radius: 3px;
  color: var(--accent);
  background: color-mix(in srgb, var(--accent) 8%, transparent);
}

main { padding: 0 var(--gap-lg) var(--gap-xl); }
.group-header {
  color: var(--ink-faint);
  font-size: 0.76rem;
  padding: var(--gap-md) var(--gap-xs) var(--gap-xs);
  text-transform: lowercase;
  letter-spacing: 0.04em;
}
.group {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto minmax(0, 2fr) auto;
  column-gap: var(--gap-md);
  row-gap: 0;
}
.row {
  display: contents;
}
.row[hidden] {
  display: none;
}
.group[hidden] {
  display: none;
}
.row > .test,
.row > .result,
.row > .details,
.row > .duration {
  padding: 8px var(--gap-sm);
  border-bottom: 1px solid var(--border);
  min-width: 0;
}
a.row:hover > .test,
a.row:hover > .result,
a.row:hover > .details,
a.row:hover > .duration {
  background: color-mix(in srgb, var(--accent) 8%, transparent);
}
.row .test {
  font-family: var(--font-mono);
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}
.row .test .name {
  color: var(--ink);
  font-size: 0.95rem;
  word-break: break-word;
}
.row .test .path {
  color: var(--ink-faint);
  font-size: 0.78rem;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.row .result {
  display: flex;
  align-items: center;
}
.row .result .pill {
  font-family: var(--font-mono);
  font-size: 0.72rem;
  letter-spacing: 0.05em;
  text-transform: uppercase;
  padding: 2px 10px;
  border: 1px solid currentColor;
  border-radius: 100px;
}
.row .result .pill.pass { color: var(--pass); }
.row .result .pill.fail { color: var(--fail); }
.row .result .pill.cancel { color: var(--cancel); }
.row .result .pill.invalid { color: var(--invalid); }
.row .result .pill.skip { color: var(--skip); }
.row .details {
  color: var(--ink-dim);
  font-family: var(--font-mono);
  font-size: 0.78rem;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  display: flex;
  align-items: center;
}
.row .details .flaky {
  font-family: var(--font-mono);
  font-size: 0.65rem;
  color: var(--accent);
  border: 1px solid var(--accent);
  border-radius: 100px;
  padding: 1px 8px;
  background: color-mix(in srgb, var(--accent) 8%, transparent);
}
.row .duration {
  color: var(--ink-dim);
  font-family: var(--font-mono);
  font-size: 0.78rem;
  text-align: right;
  white-space: nowrap;
}

mark.search-hit {
  background-color: color-mix(in srgb, var(--accent) 12%, transparent);
  color: inherit;
  border-radius: 2px;
  padding: 0;
}
mark.search-hit-current {
  background-color: color-mix(in srgb, var(--accent) 36%, transparent);
  color: var(--accent);
  border-radius: 2px;
  outline: 1px solid var(--accent);
  outline-offset: 0;
}
"#;

const FOOTER: &str = "</body></html>\n";

const SCRIPT: &str = r#"
(function () {
  function ready(fn) {
    if (document.readyState !== "loading") fn();
    else document.addEventListener("DOMContentLoaded", fn);
  }

  ready(function () {
    var input = document.querySelector('input[data-search-input]');
    if (!input) return;
    var counter = document.querySelector('.search-input .count');
    var kbdBadge = document.querySelector('.search-input .kbd');
    var isMac = /Mac|iPod|iPhone|iPad/.test(navigator.platform);
    if (kbdBadge) kbdBadge.textContent = isMac ? "\u2318S" : "Ctrl+S";

    var groups = Array.prototype.slice.call(document.querySelectorAll('.group'));
    var groupHeaders = Array.prototype.slice.call(document.querySelectorAll('.group-header'));
    var rows = Array.prototype.slice.call(document.querySelectorAll('.row'));

    rows.forEach(function (row) {
      var nameSpan = row.querySelector('.test .name');
      if (nameSpan) row.dataset.originalName = nameSpan.textContent;
    });

    var currentIndex = -1;
    var currentHits = [];

    function escapeRegex(s) {
      return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    }

    function clearHighlights() {
      rows.forEach(function (row) {
        var nameSpan = row.querySelector('.test .name');
        if (nameSpan && row.dataset.originalName !== undefined) {
          nameSpan.textContent = row.dataset.originalName;
        }
      });
    }

    function rebuildHits(query) {
      clearHighlights();
      if (query.length === 0) {
        rows.forEach(function (row) { row.hidden = false; });
        groups.forEach(function (g) { g.hidden = false; });
        groupHeaders.forEach(function (h) {
          h.hidden = false;
          var label = h.dataset.groupHeader;
          var count = document.querySelectorAll('.group[data-group="' + label + '"] .row').length;
          h.innerHTML = '\u2014 ' + label + ' <span class="count">(' + count + ')</span>';
        });
        if (counter) counter.textContent = "";
        return [];
      }

      var insensitive = query === query.toLowerCase();
      var flags = insensitive ? "gi" : "g";
      var re = new RegExp(escapeRegex(query), flags);

      var hits = [];
      var perGroupVisible = {};
      var perGroupTotal = {};
      rows.forEach(function (row) {
        var groupEl = row.closest('.group');
        if (!groupEl) return;
        var groupLabel = groupEl.dataset.group;
        if (!(groupLabel in perGroupTotal)) perGroupTotal[groupLabel] = 0;
        if (!(groupLabel in perGroupVisible)) perGroupVisible[groupLabel] = 0;
        perGroupTotal[groupLabel]++;

        var name = row.dataset.originalName || "";
        re.lastIndex = 0;
        var matches = [];
        for (var m = re.exec(name); m !== null; m = re.exec(name)) {
          matches.push({ start: m.index, end: m.index + m[0].length });
          if (m.index === re.lastIndex) re.lastIndex++;
        }
        if (matches.length === 0) {
          row.hidden = true;
          return;
        }
        row.hidden = false;
        perGroupVisible[groupLabel]++;

        var nameSpan = row.querySelector('.test .name');
        if (!nameSpan) return;
        var frag = document.createDocumentFragment();
        var pos = 0;
        matches.forEach(function (mt) {
          if (mt.start > pos) frag.appendChild(document.createTextNode(name.slice(pos, mt.start)));
          var mark = document.createElement('mark');
          mark.className = 'search-hit';
          mark.textContent = name.slice(mt.start, mt.end);
          frag.appendChild(mark);
          hits.push({ row: row, mark: mark });
          pos = mt.end;
        });
        if (pos < name.length) frag.appendChild(document.createTextNode(name.slice(pos)));
        nameSpan.textContent = '';
        nameSpan.appendChild(frag);
      });

      groupHeaders.forEach(function (h) {
        var label = h.dataset.groupHeader;
        var visible = perGroupVisible[label] || 0;
        var total = perGroupTotal[label] || 0;
        var groupEl = document.querySelector('.group[data-group="' + label + '"]');
        if (visible === 0) {
          h.hidden = true;
          if (groupEl) groupEl.hidden = true;
        } else {
          h.hidden = false;
          if (groupEl) groupEl.hidden = false;
          h.innerHTML = '\u2014 ' + label + ' <span class="count">(' + visible + ' / ' + total + ')</span>';
        }
      });

      if (counter) counter.textContent = hits.length + " / " + rows.length;
      return hits;
    }

    function markCurrent(index) {
      currentHits.forEach(function (h) { h.mark.classList.remove('search-hit-current'); });
      if (index < 0 || index >= currentHits.length) return;
      var hit = currentHits[index];
      hit.mark.classList.add('search-hit-current');
      var rect = hit.row.getBoundingClientRect();
      var top = window.scrollY + rect.top + rect.height / 2 - window.innerHeight / 2;
      var max = document.documentElement.scrollHeight - window.innerHeight;
      window.scrollTo(0, Math.max(0, Math.min(max, top)));
    }

    function recompute() {
      currentHits = rebuildHits(input.value);
      currentIndex = currentHits.length > 0 ? 0 : -1;
      markCurrent(currentIndex);
    }

    input.addEventListener('input', recompute);
    input.addEventListener('keydown', function (event) {
      if (event.key === 'Enter') {
        event.preventDefault();
        if (currentHits.length === 0) return;
        var delta = event.shiftKey ? -1 : 1;
        currentIndex = (currentIndex + delta + currentHits.length) % currentHits.length;
        markCurrent(currentIndex);
      } else if (event.key === 'Escape') {
        event.preventDefault();
        if (input.value.length > 0) {
          input.value = '';
          recompute();
        } else {
          input.blur();
        }
      }
    });

    document.addEventListener('keydown', function (event) {
      if ((event.metaKey || event.ctrlKey) && !event.altKey && !event.shiftKey && event.key.toLowerCase() === 's') {
        event.preventDefault();
        input.focus();
        input.select();
      }
    });
  });
})();
"#;

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

fn total_duration(results: &[TestResult]) -> String {
    let total: std::time::Duration = results.iter().map(|r| r.duration).sum();
    format_duration(total)
}

fn details_for(result: &TestResult) -> String {
    match &result.outcome {
        Outcome::Fail(f) => f.summary(),
        Outcome::Cancelled(c) => c.summary(),
        Outcome::Invalid(reason) | Outcome::Skipped(reason) => reason.clone(),
        Outcome::Pass => String::new(),
    }
}

fn render_group(
    html: &mut String,
    run_dir: &Path,
    label: &str,
    pill_class: &str,
    pill_label: &str,
    rows: &[&TestResult],
) {
    if rows.is_empty() {
        return;
    }
    let _ = writeln!(
        html,
        "<div class=\"group-header\" data-group-header=\"{label}\">\u{2014} {label} \
         <span class=\"count\">({count})</span></div>",
        count = rows.len(),
    );
    let _ = writeln!(html, "<div class=\"group\" data-group=\"{label}\">");
    for r in rows {
        let row_open = match &r.log_dir {
            Some(log_dir) => {
                let rel = log_dir.strip_prefix(run_dir).unwrap_or(log_dir);
                format!(
                    "<a class=\"row\" data-name=\"{name_attr}\" href=\"{href}/event.html\">",
                    name_attr = html_escape(&r.test_name),
                    href = rel.display(),
                )
            }
            None => format!(
                "<div class=\"row\" data-name=\"{name_attr}\">",
                name_attr = html_escape(&r.test_name),
            ),
        };
        let row_close = if r.log_dir.is_some() {
            "</a>"
        } else {
            "</div>"
        };

        let details = details_for(r);
        let details_cell = if matches!(r.outcome, Outcome::Pass) && r.flaky_retries > 0 {
            format!(
                "<span class=\"flaky\">flaky &middot; {}</span>",
                r.flaky_retries
            )
        } else if details.is_empty() {
            String::new()
        } else {
            html_escape(&details)
        };

        let _ = writeln!(
            html,
            "{row_open}\
             <span class=\"test\"><span class=\"name\">{name}</span>\
             <span class=\"path\">{path}</span></span>\
             <span class=\"result\"><span class=\"pill {pill_class}\">{pill_label}</span></span>\
             <span class=\"details\">{details_cell}</span>\
             <span class=\"duration\">{duration}</span>\
             {row_close}",
            name = html_escape(&r.test_name),
            path = html_escape(&r.test_path),
            duration = format_duration(r.duration),
        );
    }
    let _ = writeln!(html, "</div>");
}

pub fn generate(
    run_dir: &Path,
    results: &[TestResult],
    wall_duration: std::time::Duration,
    jobs: usize,
) {
    let html = render(run_dir, results, wall_duration, jobs);
    let path = run_dir.join("index.html");
    let _ = fs::write(path, html);
}

fn render(
    run_dir: &Path,
    results: &[TestResult],
    wall_duration: std::time::Duration,
    jobs: usize,
) -> String {
    let mut html = header("relux run summary");

    let run_name = run_dir.file_name().unwrap_or_default().to_string_lossy();
    let total = results.len();
    let flaky: u32 = results.iter().map(|r| r.flaky_retries).sum();
    let overall_ok = !results.iter().any(|r| {
        matches!(
            r.outcome,
            Outcome::Fail(_) | Outcome::Cancelled(_) | Outcome::Invalid(_)
        )
    });
    let (pill_class, pill_label) = if overall_ok {
        ("pass", "OK")
    } else {
        ("fail", "FAILED")
    };

    let flaky_seg = if flaky > 0 {
        format!(" &middot; {flaky} flaky retries")
    } else {
        String::new()
    };

    let timing = if jobs > 1 {
        format!(
            "{} <span class=\"cum\">({} cumulative in {jobs} jobs)</span>",
            format_duration(wall_duration),
            total_duration(results),
        )
    } else {
        format_duration(wall_duration)
    };

    let _ = writeln!(
        html,
        "<header class=\"appbar\">\
         <span class=\"crumbs\">runs<span class=\"sep\">/</span>\
         <span class=\"run-id\">{}</span></span>\
         <span class=\"pill {pill_class}\">{pill_label}</span>\
         <span class=\"timing\">{timing} &middot; {total} tests{flaky_seg}</span>\
         </header>",
        html_escape(&run_name),
    );

    let mut report_links = Vec::new();
    if run_dir.join("results.tap").exists() {
        report_links.push("<a href=\"results.tap\">TAP</a>");
    }
    if run_dir.join("junit.xml").exists() {
        report_links.push("<a href=\"junit.xml\">JUnit XML</a>");
    }
    if !report_links.is_empty() {
        let _ = writeln!(
            html,
            "<div class=\"reports-bar\">{}</div>",
            report_links.join(" <span class=\"sep\">&middot;</span> ")
        );
    }

    let _ = writeln!(
        html,
        "<div class=\"search-row\">\
         <div class=\"search-input\">\
         <span class=\"glyph\">&#x2315;</span>\
         <input type=\"search\" data-search-input placeholder=\"filter by test name\u{2026}\" aria-label=\"filter test rows\">\
         <span class=\"count\"></span>\
         <kbd class=\"kbd\">\u{2318}S</kbd>\
         </div>\
         </div>"
    );

    let failed_rows: Vec<&TestResult> = results
        .iter()
        .filter(|r| matches!(r.outcome, Outcome::Fail(_)))
        .collect();
    let cancelled_rows: Vec<&TestResult> = results
        .iter()
        .filter(|r| matches!(r.outcome, Outcome::Cancelled(_)))
        .collect();
    let invalid_rows: Vec<&TestResult> = results
        .iter()
        .filter(|r| matches!(r.outcome, Outcome::Invalid(_)))
        .collect();
    let skipped_rows: Vec<&TestResult> = results
        .iter()
        .filter(|r| matches!(r.outcome, Outcome::Skipped(_)))
        .collect();
    let passed_rows: Vec<&TestResult> = results
        .iter()
        .filter(|r| matches!(r.outcome, Outcome::Pass))
        .collect();

    let _ = writeln!(html, "<main>");
    render_group(&mut html, run_dir, "failed", "fail", "FAIL", &failed_rows);
    render_group(
        &mut html,
        run_dir,
        "cancelled",
        "cancel",
        "CANCEL",
        &cancelled_rows,
    );
    render_group(
        &mut html,
        run_dir,
        "invalid",
        "invalid",
        "INVALID",
        &invalid_rows,
    );
    render_group(&mut html, run_dir, "skipped", "skip", "SKIP", &skipped_rows);
    render_group(&mut html, run_dir, "passed", "pass", "PASS", &passed_rows);
    let _ = writeln!(html, "</main>");

    let _ = writeln!(html, "<script>{SCRIPT}</script>");
    html.push_str(FOOTER);
    html
}

#[cfg(test)]
mod tests {
    use crate::report::result::Failure;
    use crate::report::result::FailureContext;
    use crate::report::result::Outcome;
    use crate::report::result::TestResult;
    use std::path::PathBuf;
    use std::time::Duration;

    fn pass(name: &str) -> TestResult {
        TestResult {
            test_name: name.to_string(),
            test_path: format!("tests/{name}.relux"),
            outcome: Outcome::Pass,
            duration: Duration::from_millis(120),
            progress: "1/1".to_string(),
            log_dir: Some(PathBuf::from(format!("/tmp/run/{name}"))),
            warnings: Vec::new(),
            flaky_retries: 0,
        }
    }

    fn fail(name: &str) -> TestResult {
        let mut r = pass(name);
        r.outcome = Outcome::Fail(Failure::Runtime {
            message: "boom".into(),
            span: None,
            shell: Some("default".into()),
            context: FailureContext::pre_vm(),
        });
        r
    }

    fn skip(name: &str, reason: &str) -> TestResult {
        let mut r = pass(name);
        r.outcome = Outcome::Skipped(reason.to_string());
        r
    }

    fn invalid(name: &str, reason: &str) -> TestResult {
        let mut r = pass(name);
        r.outcome = Outcome::Invalid(reason.to_string());
        r
    }

    fn render(run_dir: &std::path::Path, results: &[TestResult]) -> String {
        super::render(run_dir, results, Duration::from_millis(50), 1)
    }

    #[test]
    fn render_contains_basic_markers() {
        let run_dir = std::path::Path::new("/tmp/run-001");
        let results = vec![pass("alpha"), fail("beta")];
        let html = render(run_dir, &results);
        assert!(html.contains("alpha"));
        assert!(html.contains("beta"));
        assert!(html.contains("<header class=\"appbar\""));
        assert!(html.contains("class=\"pill fail\""));
        assert!(html.contains("FAILED"));
        assert!(html.contains("run-001"));
    }

    #[test]
    fn render_header_ok_when_all_pass() {
        let run_dir = std::path::Path::new("/tmp/run-001");
        let results = vec![pass("alpha"), pass("beta")];
        let html = render(run_dir, &results);
        assert!(html.contains("class=\"pill pass\""));
        assert!(html.contains(">OK<"));
    }

    #[test]
    fn render_timing_shows_wall_only_when_jobs_is_one() {
        let run_dir = std::path::Path::new("/tmp/run-001");
        let html = super::render(run_dir, &[pass("alpha")], Duration::from_millis(50), 1);
        assert!(html.contains("50.0 ms"));
        assert!(!html.contains("cumulative"));
    }

    #[test]
    fn render_timing_shows_cumulative_when_jobs_gt_one() {
        let run_dir = std::path::Path::new("/tmp/run-001");
        let html = super::render(
            run_dir,
            &[pass("alpha"), pass("beta")],
            Duration::from_millis(50),
            8,
        );
        assert!(html.contains("50.0 ms"));
        assert!(html.contains("240.0 ms cumulative in 8 jobs"));
    }

    #[test]
    fn render_flaky_segment_only_when_nonzero() {
        let run_dir = std::path::Path::new("/tmp/run-001");
        let mut r = pass("alpha");
        r.flaky_retries = 2;
        let html_with = render(run_dir, &[r]);
        assert!(html_with.contains("2 flaky retries"));

        let html_without = render(run_dir, &[pass("beta")]);
        assert!(!html_without.contains("flaky retries"));
    }

    #[test]
    fn render_groups_omit_empty() {
        let run_dir = std::path::Path::new("/tmp/run-001");
        let results = vec![pass("alpha")];
        let html = render(run_dir, &results);
        assert!(html.contains("data-group=\"passed\""));
        assert!(!html.contains("data-group=\"failed\""));
        assert!(!html.contains("data-group=\"invalid\""));
        assert!(!html.contains("data-group=\"skipped\""));
    }

    #[test]
    fn render_failed_details_carry_failure_summary() {
        let run_dir = std::path::Path::new("/tmp/run-001");
        let results = vec![fail("beta")];
        let html = render(run_dir, &results);
        assert!(html.contains("runtime error in shell 'default': boom"));
    }

    #[test]
    fn render_skipped_details_carry_reason() {
        let run_dir = std::path::Path::new("/tmp/run-001");
        let results = vec![skip("gamma", "tagged @skip")];
        let html = render(run_dir, &results);
        assert!(html.contains("tagged @skip"));
        assert!(html.contains("data-group=\"skipped\""));
    }

    #[test]
    fn render_invalid_details_carry_reason() {
        let run_dir = std::path::Path::new("/tmp/run-001");
        let results = vec![invalid("delta", "could not resolve import")];
        let html = render(run_dir, &results);
        assert!(html.contains("could not resolve import"));
        assert!(html.contains("data-group=\"invalid\""));
    }

    #[test]
    fn render_passed_row_with_flaky_retries_shows_badge() {
        let run_dir = std::path::Path::new("/tmp/run-001");
        let mut r = pass("alpha");
        r.flaky_retries = 3;
        let html = render(run_dir, &[r]);
        assert!(html.contains("class=\"flaky\""));
        assert!(html.contains("flaky &middot; 3"));
    }

    #[test]
    fn render_row_is_anchor_when_log_dir_present() {
        let run_dir = std::path::Path::new("/tmp/run-001");
        let results = vec![pass("alpha")];
        let html = render(run_dir, &results);
        assert!(html.contains("<a class=\"row\""));
        assert!(html.contains("alpha/event.html"));
    }

    #[test]
    fn render_row_is_div_when_log_dir_absent() {
        let run_dir = std::path::Path::new("/tmp/run-001");
        let mut r = pass("alpha");
        r.log_dir = None;
        let html = render(run_dir, &[r]);
        assert!(html.contains("<div class=\"row\""));
        assert!(!html.contains("alpha/event.html"));
    }

    #[test]
    fn render_includes_search_input_with_data_attr() {
        let run_dir = std::path::Path::new("/tmp/run-001");
        let html = render(run_dir, &[pass("alpha")]);
        assert!(html.contains("data-search-input"));
        assert!(html.contains("class=\"kbd\""));
    }

    #[test]
    fn render_includes_inline_script() {
        let run_dir = std::path::Path::new("/tmp/run-001");
        let html = render(run_dir, &[pass("alpha")]);
        assert!(html.contains("<script>"));
        assert!(html.contains("addEventListener"));
        assert!(html.contains("data-search-input"));
    }

    #[test]
    fn render_group_order_failed_invalid_skipped_passed() {
        let run_dir = std::path::Path::new("/tmp/run-001");
        let results = vec![
            pass("p1"),
            skip("s1", "skip-reason"),
            fail("f1"),
            invalid("i1", "invalid-reason"),
        ];
        let html = render(run_dir, &results);
        let pos_failed = html.find("data-group=\"failed\"").unwrap();
        let pos_invalid = html.find("data-group=\"invalid\"").unwrap();
        let pos_skipped = html.find("data-group=\"skipped\"").unwrap();
        let pos_passed = html.find("data-group=\"passed\"").unwrap();
        assert!(pos_failed < pos_invalid);
        assert!(pos_invalid < pos_skipped);
        assert!(pos_skipped < pos_passed);
    }
}
