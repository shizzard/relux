use std::ffi::OsStr;
use std::path::PathBuf;

use clap_complete::engine::CompletionCandidate;

use relux_ast::AstItem;
use relux_ast::AstTestItem;
use relux_core::config;
use relux_parser::parse;

fn is_not_skipped_dir(entry: &walkdir::DirEntry) -> bool {
    if !entry.file_type().is_dir() {
        return true;
    }
    // Skip relux/out directories
    if entry.file_name() == config::OUT_DIR {
        return entry
            .path()
            .parent()
            .and_then(|p| p.file_name())
            .is_none_or(|n| n != config::RELUX_DIR);
    }
    true
}

fn find_relux_files(base: &std::path::Path) -> Vec<PathBuf> {
    let Ok(cwd) = std::env::current_dir() else {
        return vec![];
    };
    if !base.is_dir() {
        return vec![];
    }
    walkdir::WalkDir::new(base)
        .into_iter()
        .filter_entry(is_not_skipped_dir)
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().is_file() && e.path().extension().is_some_and(|ext| ext == "relux")
        })
        .filter_map(|e| e.path().strip_prefix(&cwd).ok().map(|p| p.to_path_buf()))
        .collect()
}

fn find_dirs(base: &std::path::Path) -> Vec<PathBuf> {
    let Ok(cwd) = std::env::current_dir() else {
        return vec![];
    };
    if !base.is_dir() {
        return vec![];
    }
    walkdir::WalkDir::new(base)
        .into_iter()
        .filter_entry(is_not_skipped_dir)
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_dir() && e.path() != base)
        .filter_map(|e| e.path().strip_prefix(&cwd).ok().map(|p| p.to_path_buf()))
        .collect()
}

pub fn complete_relux_files(_current: &OsStr) -> Vec<CompletionCandidate> {
    let Ok(cwd) = std::env::current_dir() else {
        return vec![];
    };
    find_relux_files(&cwd)
        .into_iter()
        .map(|p| CompletionCandidate::new(p.to_string_lossy().into_owned()))
        .collect()
}

pub fn complete_test_dirs(_current: &OsStr) -> Vec<CompletionCandidate> {
    let Ok((root, _)) = config::discover_project_root() else {
        return vec![];
    };
    find_dirs(&config::tests_dir(&root))
        .into_iter()
        .map(|p| CompletionCandidate::new(p.to_string_lossy().into_owned()))
        .collect()
}

pub fn complete_effect_dirs(_current: &OsStr) -> Vec<CompletionCandidate> {
    let Ok((root, _)) = config::discover_project_root() else {
        return vec![];
    };
    find_dirs(&config::lib_dir(&root))
        .into_iter()
        .map(|p| CompletionCandidate::new(p.to_string_lossy().into_owned()))
        .collect()
}

pub fn complete_manifest(_current: &OsStr) -> Vec<CompletionCandidate> {
    let Ok(cwd) = std::env::current_dir() else {
        return vec![];
    };
    walkdir::WalkDir::new(&cwd)
        .into_iter()
        .filter_entry(is_not_skipped_dir)
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().is_file() && e.file_name().to_string_lossy() == config::CONFIG_FILE
        })
        .filter_map(|e| {
            e.path()
                .strip_prefix(&cwd)
                .ok()
                .map(|p| CompletionCandidate::new(p.to_string_lossy().into_owned()))
        })
        .collect()
}

fn format_duration(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    if secs >= 3600 && secs.is_multiple_of(3600) {
        format!("{}h", secs / 3600)
    } else if secs >= 60 && secs.is_multiple_of(60) {
        format!("{}m", secs / 60)
    } else {
        format!("{secs}s")
    }
}

fn timeout_candidates(base: std::time::Duration, label: &str) -> Vec<CompletionCandidate> {
    [2, 3, 5]
        .iter()
        .map(|&m| {
            let scaled = base * m;
            CompletionCandidate::new(format_duration(scaled))
                .help(Some(format!("{label} x{m}").into()))
        })
        .collect()
}

pub fn complete_test_timeout(_current: &OsStr) -> Vec<CompletionCandidate> {
    let base = config::discover_project_root()
        .map(|(_, cfg)| cfg.timeout.test)
        .unwrap_or(config::DEFAULT_TEST_TIMEOUT);
    timeout_candidates(base, "test timeout")
}

pub fn complete_suite_timeout(_current: &OsStr) -> Vec<CompletionCandidate> {
    let base = config::discover_project_root()
        .map(|(_, cfg)| cfg.timeout.suite)
        .unwrap_or(config::DEFAULT_SUITE_TIMEOUT);
    timeout_candidates(base, "suite timeout")
}

pub fn complete_shell(_current: &OsStr) -> Vec<CompletionCandidate> {
    vec![
        CompletionCandidate::new("bash").help(Some("Bourne Again SHell".into())),
        CompletionCandidate::new("zsh").help(Some("Z SHell".into())),
        CompletionCandidate::new("fish").help(Some("Friendly Interactive SHell".into())),
    ]
}

pub fn complete_test_names(_current: &OsStr) -> Vec<CompletionCandidate> {
    // ArgValueCompleter only receives the current partial value, not other parsed args.
    // We re-parse std::env::args() with ignore_errors to access the --file value.
    //
    // Note: this calls cli() which registers this very function as a completer,
    // making it self-referential. This is safe because try_get_matches_from only
    // parses args and does not invoke completers.
    // During shell completion, clap_complete invokes the binary as
    // `<binary> -- relux run --file foo.relux --test <partial>`.
    // The args after `--` are the actual command line being completed,
    // already starting with the binary name.
    let all_args: Vec<String> = std::env::args().collect();
    let args: Vec<&str> = match all_args.iter().position(|a| a == "--") {
        Some(pos) => all_args[pos + 1..].iter().map(|s| s.as_str()).collect(),
        None => all_args.iter().map(|s| s.as_str()).collect(),
    };
    let Ok(matches) = super::cli().ignore_errors(true).try_get_matches_from(args) else {
        return vec![];
    };
    let Some(run_matches) = matches.subcommand_matches("run") else {
        return vec![];
    };
    let files: Vec<&PathBuf> = run_matches
        .get_many::<PathBuf>("paths")
        .map(|v| v.collect())
        .unwrap_or_default();
    if files.len() != 1 {
        return vec![];
    }
    let source = match std::fs::read_to_string(files[0]) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    let Ok(module) = parse(&source) else {
        return vec![];
    };
    module
        .items
        .iter()
        .filter_map(|item| match &item.node {
            AstItem::Test { def, .. } => {
                let name = def.name.node.clone();
                let help = def
                    .body
                    .iter()
                    .find_map(|item| match &item.node {
                        AstTestItem::DocString { text, .. } => text
                            .lines()
                            .find(|l| !l.trim().is_empty())
                            .map(|l| l.trim()),
                        _ => None,
                    })
                    .unwrap_or("");
                let mut candidate = CompletionCandidate::new(name);
                if !help.is_empty() {
                    candidate = candidate.help(Some(help.to_string().into()));
                }
                Some(candidate)
            }
            _ => None,
        })
        .collect()
}
