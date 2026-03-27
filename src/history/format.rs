use std::collections::HashMap;
use std::collections::HashSet;
use std::time::Duration;

use serde::Serialize;
use tabled::builder::Builder;
use tabled::settings::Alignment;
use tabled::settings::Modify;
use tabled::settings::Style;
use tabled::settings::Width;
use tabled::settings::object::Columns;
use tabled::settings::span::Span;

use crate::runtime::report::result::format_duration;

use super::analysis::DurationRecord;
use super::analysis::DurationStats;
use super::analysis::FailureModeEntry;
use super::analysis::FailureRecord;
use super::analysis::FirstFailRecord;
use super::analysis::FlakyRecord;
use super::analysis::LoadedRunsCollection;
use super::analysis::TestKey;

// ─── Shared Helpers ────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct ReportMeta {
    runs: usize,
}

fn build_file_index(test_ids: &[&str]) -> (HashMap<String, usize>, String) {
    let mut seen: Vec<String> = Vec::new();
    let mut seen_set: HashSet<String> = HashSet::new();
    for id in test_ids {
        if let Some(pos) = id.rfind('/') {
            let path = &id[..pos];
            if seen_set.insert(path.to_string()) {
                seen.push(path.to_string());
            }
        }
    }
    let map: HashMap<String, usize> = seen
        .iter()
        .enumerate()
        .map(|(i, p)| (p.clone(), i + 1))
        .collect();

    let mut legend = String::from("\n\nFiles:\n");
    for (i, path) in seen.iter().enumerate() {
        legend.push_str(&format!("  {}: {path}\n", i + 1));
    }
    (map, legend)
}

fn format_test_col(file_idx: &HashMap<String, usize>, test_id: &str) -> String {
    if let Some(pos) = test_id.rfind('/') {
        let path = &test_id[..pos];
        let slug = &test_id[pos + 1..];
        let n = file_idx.get(path).copied().unwrap_or(0);
        format!("[{n}] {slug}")
    } else {
        test_id.to_string()
    }
}

fn term_width() -> usize {
    terminal_size::terminal_size()
        .map(|(w, _)| w.0 as usize)
        .unwrap_or(80)
}

fn make_table(
    mut builder: Builder,
    truncated: Option<(usize, usize)>,
    footer: Option<&[Vec<String>]>,
) -> String {
    let cols = builder.count_columns();
    let trunc_row = if let Some((shown, total)) = truncated {
        let mut row: Vec<String> = vec![String::new(); cols];
        row[0] = format!("~~~ showing {shown} of {total} results ~~~");
        let row_idx = shown + 1;
        builder.push_record(row);
        Some(row_idx)
    } else {
        None
    };
    if let Some(rows) = footer {
        for row in rows {
            builder.push_record(row.clone());
        }
    }
    let mut table = builder.build();
    table
        .with(Style::blank())
        .with(Alignment::right())
        .with(Modify::new(Columns::first()).with(Alignment::left()));
    if let Some(row_idx) = trunc_row {
        table
            .modify((row_idx, 0), Span::column(cols as isize))
            .modify((row_idx, 0), Alignment::center());
    }
    table.with(Width::increase(term_width()));
    table.to_string()
}

fn fmt_dur(ms: f64) -> String {
    format_duration(Duration::from_secs_f64(ms / 1000.0))
}

fn fmt_dur_u64(ms: u64) -> String {
    format_duration(Duration::from_millis(ms))
}

// ─── Flaky ─────────────────────────────────────────────────────

pub fn format_flaky_human(
    coll: &LoadedRunsCollection,
    entries: &[(TestKey, FlakyRecord)],
) -> String {
    let mut out = format!("Flakiness Report ({} runs)\n\n", coll.run_count());

    if entries.is_empty() {
        out.push_str("No flaky tests detected.\n");
        return out;
    }

    let display_ids: Vec<String> = entries.iter().map(|(k, _)| k.to_string()).collect();
    let display_refs: Vec<&str> = display_ids.iter().map(|s| s.as_str()).collect();
    let (idx, legend) = build_file_index(&display_refs);

    let mut builder = Builder::default();
    builder.push_record(["Test", "Flips", "Pass", "Fail", "Rate"]);
    for (key, rec) in entries {
        let display_id = key.to_string();
        builder.push_record([
            format_test_col(&idx, &display_id),
            rec.flips.to_string(),
            rec.pass.to_string(),
            rec.fail.to_string(),
            format!("{:.1}%", rec.rate),
        ]);
    }

    out.push_str(&make_table(builder, coll.truncation(), None));
    out.push_str(&legend);
    out
}

pub fn format_flaky_toml(
    coll: &LoadedRunsCollection,
    entries: &[(TestKey, FlakyRecord)],
) -> String {
    #[derive(Serialize)]
    struct FlakyReport {
        meta: ReportMeta,
        tests: Vec<FlakyEntryOut>,
    }
    #[derive(Serialize)]
    struct FlakyEntryOut {
        path: String,
        flips: usize,
        pass: usize,
        fail: usize,
        rate: f64,
    }

    let report = FlakyReport {
        meta: ReportMeta {
            runs: coll.run_count(),
        },
        tests: entries
            .iter()
            .map(|(k, r)| FlakyEntryOut {
                path: k.to_string(),
                flips: r.flips,
                pass: r.pass,
                fail: r.fail,
                rate: r.rate,
            })
            .collect(),
    };
    toml::to_string_pretty(&report).expect("failed to serialize flaky report")
}

// ─── Failures ──────────────────────────────────────────────────

pub fn format_failures_human(
    coll: &LoadedRunsCollection,
    entries: &[(TestKey, FailureRecord)],
    modes: &[FailureModeEntry],
) -> String {
    let mut out = format!("Failure Report ({} runs)\n\n", coll.run_count());

    if entries.is_empty() {
        out.push_str("No failures detected.\n");
        return out;
    }

    let display_ids: Vec<String> = entries.iter().map(|(k, _)| k.to_string()).collect();
    let display_refs: Vec<&str> = display_ids.iter().map(|s| s.as_str()).collect();
    let (idx, legend) = build_file_index(&display_refs);

    let mut builder = Builder::default();
    builder.push_record(["Test", "Fails", "Runs", "Rate"]);
    for (key, rec) in entries {
        let display_id = key.to_string();
        builder.push_record([
            format_test_col(&idx, &display_id),
            rec.fails.to_string(),
            rec.runs.to_string(),
            format!("{:.1}%", rec.rate),
        ]);
    }

    out.push_str(&make_table(builder, coll.truncation(), None));

    if !modes.is_empty() {
        out.push_str("\n\n");
        let mut builder = Builder::default();
        builder.push_record(["Type", "Count", "%"]);
        for m in modes {
            builder.push_record([
                m.failure_type.clone(),
                m.count.to_string(),
                format!("{:.1}%", m.percentage),
            ]);
        }
        out.push_str(&make_table(builder, None, None));
    }

    out.push_str(&legend);
    out
}

pub fn format_failures_toml(
    coll: &LoadedRunsCollection,
    entries: &[(TestKey, FailureRecord)],
    modes: &[FailureModeEntry],
) -> String {
    #[derive(Serialize)]
    struct FailureReport {
        meta: ReportMeta,
        tests: Vec<FailureEntryOut>,
        modes: Vec<FailureModeOut>,
    }
    #[derive(Serialize)]
    struct FailureEntryOut {
        path: String,
        fails: usize,
        runs: usize,
        rate: f64,
    }
    #[derive(Serialize)]
    struct FailureModeOut {
        failure_type: String,
        count: usize,
        percentage: f64,
    }

    let report = FailureReport {
        meta: ReportMeta {
            runs: coll.run_count(),
        },
        tests: entries
            .iter()
            .map(|(k, r)| FailureEntryOut {
                path: k.to_string(),
                fails: r.fails,
                runs: r.runs,
                rate: r.rate,
            })
            .collect(),
        modes: modes
            .iter()
            .map(|m| FailureModeOut {
                failure_type: m.failure_type.clone(),
                count: m.count,
                percentage: m.percentage,
            })
            .collect(),
    };
    toml::to_string_pretty(&report).expect("failed to serialize failure report")
}

// ─── First-Fail ────────────────────────────────────────────────

pub fn format_first_fail_human(
    coll: &LoadedRunsCollection,
    entries: &[(TestKey, FirstFailRecord)],
) -> String {
    let mut out = format!("Latest Regressions ({} runs)\n\n", coll.run_count());

    if entries.is_empty() {
        out.push_str("No pass-to-fail transitions detected.\n");
        return out;
    }

    for (i, (key, rec)) in entries.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&format!("  Test:    {}\n", key));
        out.push_str(&format!("  Time:    {}\n", rec.timestamp));
        out.push_str(&format!("  Type:    {}\n", rec.failure_type));
        out.push_str(&format!("  Summary: {}\n", rec.summary));
        out.push_str(&format!("  Report:  {}\n", rec.report));
    }

    out
}

pub fn format_first_fail_toml(
    coll: &LoadedRunsCollection,
    entries: &[(TestKey, FirstFailRecord)],
) -> String {
    #[derive(Serialize)]
    struct FirstFailReport {
        meta: ReportMeta,
        tests: Vec<FirstFailEntryOut>,
    }
    #[derive(Serialize)]
    struct FirstFailEntryOut {
        path: String,
        report: String,
        timestamp: String,
        failure_type: String,
        summary: String,
    }

    let report = FirstFailReport {
        meta: ReportMeta {
            runs: coll.run_count(),
        },
        tests: entries
            .iter()
            .map(|(k, r)| FirstFailEntryOut {
                path: k.to_string(),
                report: r.report.clone(),
                timestamp: r.timestamp.clone(),
                failure_type: r.failure_type.clone(),
                summary: r.summary.clone(),
            })
            .collect(),
    };
    toml::to_string_pretty(&report).expect("failed to serialize first-fail report")
}

// ─── Durations ─────────────────────────────────────────────────

pub fn format_durations_human(
    coll: &LoadedRunsCollection,
    entries: &[(TestKey, DurationRecord)],
    aggregate: &DurationStats,
) -> String {
    let mut out = format!("Duration Analysis ({} runs)\n\n", coll.run_count());

    if entries.is_empty() {
        out.push_str("No duration data available.\n");
        return out;
    }

    let display_ids: Vec<String> = entries.iter().map(|(k, _)| k.to_string()).collect();
    let display_refs: Vec<&str> = display_ids.iter().map(|s| s.as_str()).collect();
    let (idx, legend) = build_file_index(&display_refs);

    let mut builder = Builder::default();
    builder.push_record(["Test", "Mean", "StdDev", "Min", "Max", "Trend"]);
    for (key, rec) in entries {
        let display_id = key.to_string();
        builder.push_record([
            format_test_col(&idx, &display_id),
            fmt_dur(rec.mean_ms),
            fmt_dur(rec.stddev_ms),
            fmt_dur_u64(rec.min_ms),
            fmt_dur_u64(rec.max_ms),
            rec.trend.clone(),
        ]);
    }

    let footer = vec![
        vec![String::new(); 6],
        vec![
            "Aggregate".to_string(),
            fmt_dur(aggregate.mean_ms),
            fmt_dur(aggregate.stddev_ms),
            fmt_dur_u64(aggregate.min_ms),
            fmt_dur_u64(aggregate.max_ms),
            aggregate.trend.clone(),
        ],
    ];

    out.push_str(&make_table(builder, coll.truncation(), Some(&footer)));
    out.push_str(&legend);
    out
}

pub fn format_durations_toml(
    coll: &LoadedRunsCollection,
    entries: &[(TestKey, DurationRecord)],
    aggregate: &DurationStats,
) -> String {
    #[derive(Serialize)]
    struct DurationReport {
        meta: ReportMeta,
        tests: Vec<DurationEntryOut>,
        aggregate: DurationStatsOut,
    }
    #[derive(Serialize)]
    struct DurationEntryOut {
        path: String,
        mean_ms: f64,
        stddev_ms: f64,
        min_ms: u64,
        max_ms: u64,
        trend: String,
    }
    #[derive(Serialize)]
    struct DurationStatsOut {
        mean_ms: f64,
        stddev_ms: f64,
        min_ms: u64,
        max_ms: u64,
        trend: String,
    }

    let report = DurationReport {
        meta: ReportMeta {
            runs: coll.run_count(),
        },
        tests: entries
            .iter()
            .map(|(k, r)| DurationEntryOut {
                path: k.to_string(),
                mean_ms: r.mean_ms,
                stddev_ms: r.stddev_ms,
                min_ms: r.min_ms,
                max_ms: r.max_ms,
                trend: r.trend.clone(),
            })
            .collect(),
        aggregate: DurationStatsOut {
            mean_ms: aggregate.mean_ms,
            stddev_ms: aggregate.stddev_ms,
            min_ms: aggregate.min_ms,
            max_ms: aggregate.max_ms,
            trend: aggregate.trend.clone(),
        },
    };
    toml::to_string_pretty(&report).expect("failed to serialize duration report")
}

// ─── Tests ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::super::analysis::DurationAggregate;
    use super::super::analysis::DurationPreaggregate;
    use super::super::analysis::FlakyPreaggregate;
    use super::super::analysis::LoadedRunsCollection;
    use super::super::analysis::tests::sample_runs;
    use super::*;

    #[test]
    fn format_flaky_human_has_legend() {
        let runs = sample_runs();
        let mut coll = LoadedRunsCollection::new(runs);
        let entries = coll.truncate::<FlakyPreaggregate>(None);
        let output = format_flaky_human(&coll, &entries);
        assert!(output.contains("Flakiness Report (4 runs)"));
        assert!(output.contains("[1]") || output.contains("[2]") || output.contains("[3]"));
        assert!(output.contains("Files:"));
        assert!(output.contains("a.relux"));
    }

    #[test]
    fn format_durations_human_has_aggregate() {
        let runs = sample_runs();
        let mut coll = LoadedRunsCollection::new(runs);
        let entries = coll.truncate::<DurationPreaggregate>(None);
        let aggregate = coll.aggregate::<DurationAggregate>();
        let output = format_durations_human(&coll, &entries, &aggregate);
        assert!(output.contains("Aggregate"));
        assert!(output.contains("Duration Analysis (4 runs)"));
        assert!(output.contains("Files:"));
    }

    #[test]
    fn build_file_index_assigns_sequential_numbers() {
        let ids = vec![
            "foo/bar.relux/test-one",
            "foo/bar.relux/test-two",
            "baz.relux/test-three",
        ];
        let (idx, legend) = build_file_index(&ids);
        assert_eq!(idx["foo/bar.relux"], 1);
        assert_eq!(idx["baz.relux"], 2);
        assert!(legend.contains("1: foo/bar.relux"));
        assert!(legend.contains("2: baz.relux"));
    }

    #[test]
    fn format_test_col_produces_bracket_format() {
        let mut idx = HashMap::new();
        idx.insert("foo.relux".to_string(), 3);
        assert_eq!(format_test_col(&idx, "foo.relux/my-test"), "[3] my-test");
    }
}
