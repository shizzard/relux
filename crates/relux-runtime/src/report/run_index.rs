//! Per-run `index.html` generator. Lists every test in a run with its
//! outcome, duration, progress string, and (when available) a link to the
//! per-test artifact directory. Self-contained — no event-log dependencies.
//!
//! The page chrome (CSS / search-and-filter script) lives in the sibling
//! `run_index.css` and `run_index.js` files. They are embedded via
//! `include_str!` so IDE / lint tooling can apply CSS / JS analyses to
//! them instead of treating them as opaque Rust string literals.

use std::fmt::Write as FmtWrite;
use std::fs;
use std::path::Path;

use crate::report::result::Outcome;
use crate::report::result::TestResult;
use crate::report::result::format_duration;

/// Stylesheet inlined into the `<style>` block of `index.html`.
const CSS: &str = include_str!("run_index.css");

/// Filter / search script inlined into the trailing `<script>` block of
/// `index.html`.
const SCRIPT: &str = include_str!("run_index.js");

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
