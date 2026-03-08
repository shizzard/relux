use std::path::Path;

use quick_junit::{NonSuccessKind, Property, Report, TestCase, TestCaseStatus, TestSuite};

use crate::dsl::resolver::ir::{SourceMap, Span};
use crate::runtime::result::{log_link, Failure, Outcome, TestResult};

pub fn generate_junit(
    run_dir: &Path,
    suite_name: &str,
    results: &[TestResult],
    source_map: &SourceMap,
) {
    let xml = render_junit(suite_name, results, run_dir, source_map);
    std::fs::write(run_dir.join("junit.xml"), xml).expect("failed to write junit.xml");
}

fn render_junit(
    suite_name: &str,
    results: &[TestResult],
    run_dir: &Path,
    source_map: &SourceMap,
) -> String {
    let mut report = Report::new(suite_name);
    let mut suite = TestSuite::new(suite_name);

    for result in results {
        let classname = Path::new(&result.test_path)
            .with_extension("")
            .display()
            .to_string();
        let mut case = TestCase::new(&result.test_name, TestCaseStatus::success());
        case.set_classname(&classname);
        case.set_time(result.duration);

        match &result.outcome {
            Outcome::Pass => {} // status already success
            Outcome::Fail(failure) => {
                let mut status = TestCaseStatus::non_success(NonSuccessKind::Failure);
                status.set_message(failure.summary());
                status.set_type(failure.failure_type());
                status.set_description(format_failure_detail(failure, source_map));
                case.status = status;
            }
            Outcome::Skipped(reason) => {
                let mut status = TestCaseStatus::skipped();
                status.set_message(reason.as_str());
                case.status = status;
            }
        }

        if let Some(link) = log_link(run_dir, result) {
            case.set_system_out(format!("[[ATTACHMENT|{link}]]"));
            case.add_property(Property::new("log", &link));
        }

        suite.add_test_case(case);
    }

    report.add_test_suite(suite);
    report.to_string().expect("JUnit XML serialization failed")
}

fn format_failure_detail(failure: &Failure, source_map: &SourceMap) -> String {
    match failure {
        Failure::MatchTimeout {
            pattern,
            span,
            shell,
        } => {
            let loc = source_location(span, source_map);
            format!("shell: {shell}\npattern: {pattern}\n{loc}")
        }
        Failure::FailPatternMatched {
            pattern,
            matched_line,
            span,
            shell,
        } => {
            let loc = source_location(span, source_map);
            format!("shell: {shell}\npattern: {pattern}\nmatched: {matched_line}\n{loc}")
        }
        Failure::NegativeMatchFailed {
            pattern,
            matched_text,
            span,
            shell,
        } => {
            let loc = source_location(span, source_map);
            format!("shell: {shell}\npattern: {pattern}\nmatched: {matched_text}\n{loc}")
        }
        Failure::ShellExited {
            shell,
            exit_code,
            span,
        } => {
            let loc = source_location(span, source_map);
            let code_str = match exit_code {
                Some(code) => code.to_string(),
                None => "unknown".to_string(),
            };
            format!("shell: {shell}\nexit_code: {code_str}\n{loc}")
        }
        Failure::Runtime {
            message,
            span,
            shell,
        } => {
            let shell_line = match shell {
                Some(s) => format!("shell: {s}\n"),
                None => String::new(),
            };
            let loc_line = match span {
                Some(s) => format!("\n{}", source_location(s, source_map)),
                None => String::new(),
            };
            format!("{shell_line}message: {message}{loc_line}")
        }
    }
}

fn source_location(span: &Span, source_map: &SourceMap) -> String {
    let file = &source_map.files[span.file];
    let line = line_number(&file.source, span.range.start);
    format!("file: {}\nline: {line}", file.path.display())
}

fn line_number(source: &str, offset: usize) -> usize {
    source[..offset.min(source.len())]
        .bytes()
        .filter(|&b| b == b'\n')
        .count()
        + 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::resolver::ir::{SourceFile, SourceMap, Span};
    use crate::runtime::result::{Failure, Outcome, TestResult};
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::time::Duration;

    fn test_source_map() -> SourceMap {
        SourceMap {
            files: vec![SourceFile {
                path: PathBuf::from("tests/auth/login.relux"),
                source: "line1\nline2\nline3\n".to_string(),
            }],
        }
    }

    fn make_result(
        name: &str,
        path: &str,
        outcome: Outcome,
        duration: Duration,
        log_dir: Option<PathBuf>,
    ) -> TestResult {
        TestResult {
            test_name: name.to_string(),
            test_path: path.to_string(),
            outcome,
            duration,
            shell_logs: HashMap::new(),
            progress: String::new(),
            log_dir,
        }
    }

    #[test]
    fn passed_test_with_log() {
        let source_map = test_source_map();
        let run_dir = Path::new("/tmp/runs/run-001");
        let results = vec![make_result(
            "login test",
            "tests/auth/login.relux",
            Outcome::Pass,
            Duration::from_millis(1230),
            Some(PathBuf::from("/tmp/runs/run-001/login-test")),
        )];

        let xml = render_junit("my-project", &results, run_dir, &source_map);

        assert!(xml.contains("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
        assert!(xml.contains("name=\"login test\""));
        assert!(xml.contains("classname=\"tests/auth/login\""));
        assert!(xml.contains("[[ATTACHMENT|login-test/event.html]]"));
        assert!(xml.contains("<property name=\"log\" value=\"login-test/event.html\""));
        // Should not contain failure or skipped elements
        assert!(!xml.contains("<failure"));
        assert!(!xml.contains("<skipped"));
    }

    #[test]
    fn failed_test_with_diagnostics() {
        let source_map = test_source_map();
        let run_dir = Path::new("/tmp/runs/run-001");
        // Span pointing to line 3 (offset 12 is after two newlines)
        let failure = Failure::MatchTimeout {
            pattern: "/ready/".to_string(),
            span: Span::new(0, 12..17),
            shell: "default".to_string(),
        };
        let results = vec![make_result(
            "timeout test",
            "tests/auth/timeout.relux",
            Outcome::Fail(failure),
            Duration::from_secs(5),
            Some(PathBuf::from("/tmp/runs/run-001/timeout-test")),
        )];

        let xml = render_junit("my-project", &results, run_dir, &source_map);

        assert!(xml.contains("name=\"timeout test\""));
        assert!(xml.contains("classname=\"tests/auth/timeout\""));
        assert!(xml.contains("type=\"MatchTimeout\""));
        assert!(xml.contains("shell: default"));
        assert!(xml.contains("pattern: /ready/"));
        assert!(xml.contains("file: tests/auth/login.relux"));
        assert!(xml.contains("line: 3"));
        assert!(xml.contains("[[ATTACHMENT|timeout-test/event.html]]"));
    }

    #[test]
    fn skipped_test() {
        let source_map = test_source_map();
        let run_dir = Path::new("/tmp/runs/run-001");
        let results = vec![make_result(
            "setup test",
            "tests/platform/setup.relux",
            Outcome::Skipped("os:linux".to_string()),
            Duration::ZERO,
            None,
        )];

        let xml = render_junit("my-project", &results, run_dir, &source_map);

        assert!(xml.contains("name=\"setup test\""));
        assert!(xml.contains("classname=\"tests/platform/setup\""));
        assert!(xml.contains("<skipped"));
        assert!(xml.contains("os:linux"));
        // No log link for test without log_dir
        assert!(!xml.contains("ATTACHMENT"));
        assert!(!xml.contains("<property"));
    }

    #[test]
    fn suite_name_appears_in_output() {
        let source_map = test_source_map();
        let run_dir = Path::new("/tmp/runs/run-001");
        let results = vec![];

        let xml = render_junit("my-cool-project", &results, run_dir, &source_map);

        assert!(xml.contains("name=\"my-cool-project\""));
    }

    #[test]
    fn classname_strips_extension() {
        let source_map = test_source_map();
        let run_dir = Path::new("/tmp/runs/run-001");
        let results = vec![make_result(
            "a test",
            "tests/deep/nested/file.relux",
            Outcome::Pass,
            Duration::from_millis(100),
            None,
        )];

        let xml = render_junit("proj", &results, run_dir, &source_map);

        assert!(xml.contains("classname=\"tests/deep/nested/file\""));
    }

    #[test]
    fn line_number_calculation() {
        assert_eq!(line_number("hello", 0), 1);
        assert_eq!(line_number("hello\nworld", 6), 2);
        assert_eq!(line_number("a\nb\nc\n", 4), 3);
        // Offset beyond source length is clamped
        assert_eq!(line_number("ab", 100), 1);
    }
}
