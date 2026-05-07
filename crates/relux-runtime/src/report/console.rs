use colored::Colorize;

use crate::observe::structured::StackFrame;

const ARG_VALUE_MAX: usize = 60;
const ELLIPSIS: char = '\u{2026}';

// ─── Public API ─────────────────────────────────────────────

pub fn format_call_stack(frames: &[StackFrame]) -> Option<String> {
    if frames.is_empty() {
        return None;
    }
    let mut out = format!("  {}", "Call stack:".bold());
    for frame in frames.iter().rev() {
        out.push('\n');
        out.push_str(&format_frame(frame));
    }
    Some(out)
}

pub fn format_buffer_tail(tail: &str, max_lines: usize) -> Option<String> {
    let mut lines: Vec<&str> = tail.split('\n').map(strip_trailing_cr).collect();
    if matches!(lines.last(), Some(last) if last.is_empty()) {
        lines.pop();
    }
    if lines.iter().all(|l| l.trim().is_empty()) {
        return None;
    }
    let truncated = lines.len() > max_lines;
    let shown = if truncated {
        &lines[lines.len() - max_lines..]
    } else {
        &lines[..]
    };
    let header = if truncated {
        format!("Last output (last {max_lines} lines):")
    } else {
        "Last output:".to_string()
    };
    let mut out = format!("  {}", header.bold());
    for line in shown {
        out.push('\n');
        out.push_str(&format!("    {line}").dimmed().to_string());
    }
    Some(out)
}

pub fn format_vars_in_scope(vars: &[(String, String)]) -> Option<String> {
    if vars.is_empty() {
        return None;
    }
    let mut out = format!("  {}", "Vars in scope:".bold());
    for (k, v) in vars {
        out.push('\n');
        let prefix = format!("{k} =").dimmed();
        out.push_str(&format!("    {prefix} {v:?}"));
    }
    Some(out)
}

// ─── Helpers ────────────────────────────────────────────────

fn format_frame(frame: &StackFrame) -> String {
    let body = format_frame_body(frame);
    match &frame.location {
        Some(loc) => format!("    {body}\n      {}", loc.to_string().dimmed()),
        None => format!("    {body}"),
    }
}

fn format_frame_body(frame: &StackFrame) -> String {
    let kind = frame.kind.as_str();
    match (kind, &frame.name) {
        ("fn-call", Some(name)) => {
            if frame.args.is_empty() {
                format!("call {name}")
            } else {
                format!("call {name}({})", format_args_pairs(&frame.args))
            }
        }
        ("shell-block", Some(name)) => format!("in shell '{name}'"),
        ("effect-setup", Some(name)) => {
            let alias_part = match &frame.alias {
                Some(alias) => format!(" (as '{alias}')"),
                None => String::new(),
            };
            if frame.args.is_empty() {
                format!("in effect '{name}'{alias_part}")
            } else {
                format!(
                    "in effect '{name}'({}){alias_part}",
                    format_args_pairs(&frame.args)
                )
            }
        }
        ("effect-cleanup", Some(name)) => format!("in effect-cleanup '{name}'"),
        ("test", Some(name)) => format!("in test '{name}'"),
        ("test", None) => "in test".to_string(),
        ("cleanup-block", _) => "in cleanup".to_string(),
        (kind, Some(name)) => format!("in {kind} '{name}'"),
        (kind, None) => format!("in {kind}"),
    }
}

fn format_args_pairs(args: &[(String, String)]) -> String {
    args.iter()
        .map(|(k, v)| format!("{k}={}", quoted_truncated(v)))
        .collect::<Vec<_>>()
        .join(", ")
}

fn quoted_truncated(value: &str) -> String {
    let first_line = value.split_once('\n');
    let (head, multi_line) = match first_line {
        Some((before, _)) => (before, true),
        None => (value, false),
    };
    let mut s = String::with_capacity(head.len() + 4);
    s.push('"');
    let char_count = head.chars().count();
    if char_count > ARG_VALUE_MAX {
        let head: String = head.chars().take(ARG_VALUE_MAX - 1).collect();
        s.push_str(&head);
        s.push(ELLIPSIS);
    } else {
        s.push_str(head);
        if multi_line {
            s.push(ELLIPSIS);
        }
    }
    s.push('"');
    s
}

fn strip_trailing_cr(s: &str) -> &str {
    s.strip_suffix('\r').unwrap_or(s)
}

// ─── Tests ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observe::structured::SourceLocation;

    fn frame(
        kind: &str,
        name: Option<&str>,
        args: &[(&str, &str)],
        location: Option<(&str, usize)>,
    ) -> StackFrame {
        frame_with_alias(kind, name, args, None, location)
    }

    fn frame_with_alias(
        kind: &str,
        name: Option<&str>,
        args: &[(&str, &str)],
        alias: Option<&str>,
        location: Option<(&str, usize)>,
    ) -> StackFrame {
        StackFrame {
            span: 0,
            kind: kind.to_string(),
            name: name.map(|s| s.to_string()),
            args: args
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            alias: alias.map(|s| s.to_string()),
            location: location.map(|(file, line)| SourceLocation {
                file: file.to_string(),
                line,
            }),
        }
    }

    fn force_no_color() {
        colored::control::set_override(false);
    }

    // Call stack ─────────────────────────────────────────────

    #[test]
    fn call_stack_empty_returns_none() {
        force_no_color();
        assert!(format_call_stack(&[]).is_none());
    }

    #[test]
    fn call_stack_renders_leaf_first() {
        force_no_color();
        let root = frame("test", None, &[], Some(("tests/api.relux", 3)));
        let mid = frame(
            "shell-block",
            Some("default"),
            &[],
            Some(("tests/api.relux", 5)),
        );
        let leaf = frame(
            "fn-call",
            Some("check_status"),
            &[("expected", "200")],
            Some(("lib/api.relux", 42)),
        );
        let out = format_call_stack(&[root, mid, leaf]).unwrap();
        let expected = "  Call stack:\n    call check_status(expected=\"200\")\n      lib/api.relux:42\n    in shell 'default'\n      tests/api.relux:5\n    in test\n      tests/api.relux:3";
        assert_eq!(out, expected);
    }

    #[test]
    fn call_stack_omits_empty_args() {
        force_no_color();
        let f = frame("fn-call", Some("noop"), &[], Some(("lib/util.relux", 1)));
        let out = format_call_stack(&[f]).unwrap();
        let expected = "  Call stack:\n    call noop\n      lib/util.relux:1";
        assert_eq!(out, expected);
    }

    #[test]
    fn call_stack_omits_location_when_absent() {
        force_no_color();
        let f = frame("test", None, &[], None);
        let out = format_call_stack(&[f]).unwrap();
        assert_eq!(out, "  Call stack:\n    in test");
    }

    #[test]
    fn call_stack_truncates_long_arg_value() {
        force_no_color();
        let long = "x".repeat(200);
        let f = frame("fn-call", Some("f"), &[("body", long.as_str())], None);
        let out = format_call_stack(&[f]).unwrap();
        let expected_head: String = std::iter::repeat_n('x', ARG_VALUE_MAX - 1).collect();
        assert!(
            out.contains(&format!("body=\"{expected_head}{ELLIPSIS}\"")),
            "got: {out}"
        );
    }

    #[test]
    fn call_stack_renders_effect_setup_with_alias_and_overlay() {
        force_no_color();
        let f = frame_with_alias(
            "effect-setup",
            Some("FailingService"),
            &[("URL", "http://x")],
            Some("Svc"),
            Some(("relux/tests/effect_smoke.relux", 4)),
        );
        let out = format_call_stack(&[f]).unwrap();
        let expected = "  Call stack:\n    in effect 'FailingService'(URL=\"http://x\") (as 'Svc')\n      relux/tests/effect_smoke.relux:4";
        assert_eq!(out, expected);
    }

    #[test]
    fn call_stack_renders_effect_setup_without_alias() {
        force_no_color();
        let f = frame_with_alias(
            "effect-setup",
            Some("Db"),
            &[],
            None,
            Some(("relux/tests/t.relux", 1)),
        );
        let out = format_call_stack(&[f]).unwrap();
        let expected = "  Call stack:\n    in effect 'Db'\n      relux/tests/t.relux:1";
        assert_eq!(out, expected);
    }

    #[test]
    fn call_stack_collapses_multi_line_arg_value() {
        force_no_color();
        let f = frame(
            "fn-call",
            Some("f"),
            &[("body", "first\nsecond\nthird")],
            None,
        );
        let out = format_call_stack(&[f]).unwrap();
        assert!(
            out.contains(&format!("body=\"first{ELLIPSIS}\"")),
            "got: {out}"
        );
    }

    // Buffer tail ────────────────────────────────────────────

    #[test]
    fn buffer_tail_empty_returns_none() {
        force_no_color();
        assert!(format_buffer_tail("", 12).is_none());
        assert!(format_buffer_tail("   \n  \n", 12).is_none());
    }

    #[test]
    fn buffer_tail_strips_crlf_endings() {
        force_no_color();
        let tail = "$ echo hi\r\nhi\r\nrelux> \r\n";
        let out = format_buffer_tail(tail, 12).unwrap();
        let expected = "  Last output:\n    $ echo hi\n    hi\n    relux> ";
        assert_eq!(out, expected);
    }

    #[test]
    fn buffer_tail_trailing_newline_no_phantom_blank_line() {
        force_no_color();
        let tail = "one\ntwo\n";
        let out = format_buffer_tail(tail, 12).unwrap();
        assert_eq!(out, "  Last output:\n    one\n    two");
    }

    #[test]
    fn buffer_tail_truncates_above_max_lines() {
        force_no_color();
        let tail = (1..=15)
            .map(|n| format!("line {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        let out = format_buffer_tail(&tail, 5).unwrap();
        assert!(out.starts_with("  Last output (last 5 lines):"));
        assert!(out.contains("    line 15"));
        assert!(out.contains("    line 11"));
        assert!(!out.contains("    line 10"));
    }

    #[test]
    fn buffer_tail_below_threshold_uses_plain_header() {
        force_no_color();
        let out = format_buffer_tail("a\nb", 12).unwrap();
        assert!(out.starts_with("  Last output:\n"));
        assert!(!out.contains("(last"));
    }

    // Vars in scope ──────────────────────────────────────────

    #[test]
    fn vars_in_scope_empty_returns_none() {
        force_no_color();
        assert!(format_vars_in_scope(&[]).is_none());
    }

    #[test]
    fn vars_in_scope_uses_debug_formatting_for_values() {
        force_no_color();
        let vars = vec![
            ("expected".to_string(), "200".to_string()),
            ("note".to_string(), "line one\nline two".to_string()),
        ];
        let out = format_vars_in_scope(&vars).unwrap();
        let expected =
            "  Vars in scope:\n    expected = \"200\"\n    note = \"line one\\nline two\"";
        assert_eq!(out, expected);
    }

    #[test]
    fn vars_in_scope_preserves_input_order() {
        force_no_color();
        let vars = vec![
            ("z".to_string(), "1".to_string()),
            ("a".to_string(), "2".to_string()),
        ];
        let out = format_vars_in_scope(&vars).unwrap();
        let z_pos = out.find("z =").unwrap();
        let a_pos = out.find("a =").unwrap();
        assert!(z_pos < a_pos);
    }
}
