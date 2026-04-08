use std::path::PathBuf;

use crate::history::HistoryCommand;
use crate::history::OutputFormat;
use crate::history::run_history;

use super::resolve_project;

pub fn cmd_history(matches: &clap::ArgMatches) {
    let (project_root, _config) = resolve_project(matches);

    let command = if matches.get_flag("flaky") {
        HistoryCommand::Flaky
    } else if matches.get_flag("failures") {
        HistoryCommand::Failures
    } else if matches.get_flag("first-fail") {
        HistoryCommand::FirstFail
    } else {
        HistoryCommand::Durations
    };

    let test_paths: Vec<PathBuf> = matches
        .get_many::<PathBuf>("tests")
        .map(|p| p.cloned().collect())
        .unwrap_or_default();

    let last_n: Option<usize> = matches.get_one::<usize>("last").copied();
    let top_n: Option<usize> = matches.get_one::<usize>("top").copied();

    let format = match matches.get_one::<String>("format").map(|s| s.as_str()) {
        Some("toml") => OutputFormat::Toml,
        _ => OutputFormat::Human,
    };

    run_history(&project_root, command, &test_paths, last_n, top_n, format);
}
