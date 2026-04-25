pub mod check;
pub mod completer;
pub mod completions;
pub mod dump;
pub mod history;
pub mod init;
pub mod new;
pub mod run;

use std::fs;
use std::path::PathBuf;
use std::process;

use clap::Arg;
use clap::ArgAction;
use clap::Command;
use clap::value_parser;
use clap_complete::engine::ArgValueCompleter;

use relux_core::config;
use relux_core::config::ReluxConfig;
use relux_core::diagnostics::ModulePath;
use relux_resolver::FsSourceLoader;
use relux_resolver::discover_test_modules;

pub enum ModuleKind {
    Test,
    Effect,
    Lib,
}

pub fn cli() -> Command {
    Command::new("relux")
        .about("Relux test runner")
        .version(env!("RELUX_VERSION"))
        .subcommand_required(true)
        .subcommand(
            Command::new("init").about("Initialize a new Relux project in the current directory"),
        )
        .subcommand(
            Command::new("new")
                .about("Scaffold a new test, effect, or library module")
                .group(
                    clap::ArgGroup::new("kind")
                        .args(["test", "effect", "lib"])
                        .required(true),
                )
                .arg(
                    Arg::new("test")
                        .long("test")
                        .help("Create a test module (e.g. foo/bar/baz)")
                        .value_name("MODULE_PATH")
                        .add(ArgValueCompleter::new(completer::complete_test_dirs)),
                )
                .arg(
                    Arg::new("effect")
                        .long("effect")
                        .help("Create an effect module (e.g. foo/bar/baz)")
                        .value_name("MODULE_PATH")
                        .add(ArgValueCompleter::new(completer::complete_effect_dirs)),
                )
                .arg(
                    Arg::new("lib")
                        .long("lib")
                        .help("Create a library module (e.g. utils/helpers)")
                        .value_name("MODULE_PATH")
                        .add(ArgValueCompleter::new(completer::complete_lib_dirs)),
                ),
        )
        .subcommand(
            Command::new("run")
                .about("Run tests")
                .arg(
                    Arg::new("paths")
                        .short('f')
                        .long("file")
                        .help("Test files or directories to run (default: relux/tests/)")
                        .action(ArgAction::Append)
                        .value_parser(value_parser!(PathBuf))
                        .add(ArgValueCompleter::new(completer::complete_relux_files)),
                )
                .arg(
                    Arg::new("test")
                        .short('t')
                        .long("test")
                        .help("Run only tests with this name (requires exactly one --file)")
                        .action(ArgAction::Append)
                        .add(ArgValueCompleter::new(completer::complete_test_names)),
                )
                .arg(
                    Arg::new("tap")
                        .long("tap")
                        .help("Generate TAP artifact file")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("junit")
                        .long("junit")
                        .help("Generate JUnit XML artifact file")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("multiplier")
                        .short('m')
                        .long("timeout-multiplier")
                        .help(
                            "Scale tolerance (~) timeouts by this factor; \
                             assertion (@) timeouts are not scaled",
                        )
                        .value_parser(value_parser!(f64))
                        .default_value("1.0"),
                )
                .arg(
                    Arg::new("progress")
                        .long("progress")
                        .help(
                            "Progress display mode: auto (TUI if TTY), \
                             plain (results only), tui (force TUI)",
                        )
                        .value_parser(["auto", "plain", "tui"])
                        .default_value("auto"),
                )
                .arg(
                    Arg::new("strategy")
                        .long("strategy")
                        .help(
                            "Run strategy: 'all' runs every test; \
                             'fail-fast' stops at first failure",
                        )
                        .value_parser(["all", "fail-fast"])
                        .default_value("all"),
                )
                .arg(
                    Arg::new("rerun")
                        .long("rerun")
                        .help("Re-run only non-passing tests from the latest run")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("manifest")
                        .long("manifest")
                        .help("Path to the suite manifest file (default: auto-discover Relux.toml)")
                        .value_parser(value_parser!(PathBuf))
                        .add(ArgValueCompleter::new(completer::complete_manifest)),
                )
                .arg(
                    Arg::new("flaky-retries")
                        .long("flaky-retries")
                        .help("Maximum number of retries for flaky-marked tests")
                        .value_parser(clap::value_parser!(u32)),
                )
                .arg(
                    Arg::new("flaky-multiplier")
                        .long("flaky-multiplier")
                        .help(
                            "Exponential timeout multiplier base for flaky retries (default: 1.5)",
                        )
                        .value_parser(|s: &str| {
                            let v: f64 = s
                                .parse()
                                .map_err(|e: std::num::ParseFloatError| e.to_string())?;
                            if v <= 1.0 {
                                Err("multiplier must be greater than 1.0".to_string())
                            } else {
                                Ok(v)
                            }
                        }),
                )
                .arg(
                    Arg::new("jobs")
                        .short('j')
                        .long("jobs")
                        .help("Number of parallel test workers (default: 1)")
                        .value_parser(clap::value_parser!(usize)),
                )
                .arg(
                    Arg::new("test-timeout")
                        .long("test-timeout")
                        .help("Override per-test timeout (humantime string, e.g. '5m', '30s')")
                        .value_name("DURATION")
                        .add(ArgValueCompleter::new(completer::complete_test_timeout)),
                )
                .arg(
                    Arg::new("suite-timeout")
                        .long("suite-timeout")
                        .help("Override suite timeout (humantime string, e.g. '1h', '30m')")
                        .value_name("DURATION")
                        .add(ArgValueCompleter::new(completer::complete_suite_timeout)),
                )
                .args(debug_args()),
        )
        .subcommand(
            Command::new("check")
                .about("Validate test files without executing")
                .arg(
                    Arg::new("paths")
                        .help("Test files or directories to check (default: relux/tests/)")
                        .num_args(0..)
                        .value_parser(value_parser!(PathBuf))
                        .add(ArgValueCompleter::new(completer::complete_relux_files)),
                )
                .arg(
                    Arg::new("manifest")
                        .long("manifest")
                        .help("Path to the suite manifest file (default: auto-discover Relux.toml)")
                        .value_parser(value_parser!(PathBuf))
                        .add(ArgValueCompleter::new(completer::complete_manifest)),
                ),
        )
        .subcommand(
            Command::new("history")
                .about("Analyze run history")
                .group(
                    clap::ArgGroup::new("analysis")
                        .args(["flaky", "failures", "first-fail", "durations"])
                        .required(true),
                )
                .arg(
                    Arg::new("flaky")
                        .long("flaky")
                        .help("Show flakiness rate per test")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("failures")
                        .long("failures")
                        .help("Show failure frequency and mode distribution")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("first-fail")
                        .long("first-fail")
                        .help("Show most recent pass-to-fail regression per test")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("durations")
                        .long("durations")
                        .help("Show duration trends and statistics")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("tests")
                        .long("tests")
                        .help("Filter to specific test files or directories")
                        .num_args(1..)
                        .value_parser(value_parser!(PathBuf))
                        .add(ArgValueCompleter::new(completer::complete_relux_files)),
                )
                .arg(
                    Arg::new("last")
                        .long("last")
                        .help("Limit analysis to the N most recent runs")
                        .value_parser(value_parser!(usize)),
                )
                .arg(
                    Arg::new("top")
                        .long("top")
                        .help("Show only the top N results")
                        .value_parser(value_parser!(usize)),
                )
                .arg(
                    Arg::new("format")
                        .long("format")
                        .help("Output format")
                        .value_parser(["human", "toml"])
                        .default_value("human"),
                )
                .arg(
                    Arg::new("manifest")
                        .long("manifest")
                        .help("Path to the suite manifest file (default: auto-discover Relux.toml)")
                        .value_parser(value_parser!(PathBuf))
                        .add(ArgValueCompleter::new(completer::complete_manifest)),
                ),
        )
        .subcommand(
            Command::new("completions")
                .about("Install shell completions")
                .arg(
                    Arg::new("shell")
                        .long("shell")
                        .help("Shell to generate completions for (default: autodetect from $SHELL)")
                        .value_parser(["bash", "zsh", "fish"])
                        .add(ArgValueCompleter::new(completer::complete_shell)),
                )
                .arg(
                    Arg::new("install")
                        .long("install")
                        .help("Write the completion script to the target location")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("path")
                        .long("path")
                        .help("Override the install path for the completion script")
                        .value_parser(value_parser!(PathBuf)),
                ),
        )
        .subcommand(
            Command::new("dump")
                .about("Introspection tools")
                .subcommand_required(true)
                .subcommand(
                    Command::new("tokens").about("Dump lexer tokens").arg(
                        Arg::new("file")
                            .help("File to tokenize")
                            .required(true)
                            .value_parser(value_parser!(PathBuf))
                            .add(ArgValueCompleter::new(completer::complete_relux_files)),
                    ),
                )
                .subcommand(
                    Command::new("ast").about("Dump parsed AST").arg(
                        Arg::new("file")
                            .help("File to parse")
                            .required(true)
                            .value_parser(value_parser!(PathBuf))
                            .add(ArgValueCompleter::new(completer::complete_relux_files)),
                    ),
                )
                .subcommand(
                    Command::new("ir").about("Dump resolved IR").arg(
                        Arg::new("files")
                            .help("Files to resolve")
                            .required(true)
                            .num_args(1..)
                            .value_parser(value_parser!(PathBuf))
                            .add(ArgValueCompleter::new(completer::complete_relux_files)),
                    ),
                ),
        )
}

// ---------------------------------------------------------------------------
// Debug args (feature-gated)
// ---------------------------------------------------------------------------

#[cfg(feature = "interactive-debugger")]
fn debug_args() -> Vec<Arg> {
    vec![
        Arg::new("debug")
            .long("debug")
            .help("Start interactive debugger instead of running tests")
            .action(ArgAction::SetTrue),
        Arg::new("port")
            .long("port")
            .help("WebSocket port for debug server")
            .value_parser(value_parser!(u16))
            .default_value("9377")
            .requires("debug"),
        Arg::new("log-level")
            .long("log-level")
            .help("Debug server log level")
            .value_parser(clap::builder::EnumValueParser::<relux_debug::LogLevel>::new())
            .default_value("info")
            .requires("debug"),
    ]
}

#[cfg(not(feature = "interactive-debugger"))]
fn debug_args() -> Vec<Arg> {
    vec![]
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

pub fn resolve_project(matches: &clap::ArgMatches) -> (PathBuf, ReluxConfig) {
    let result = match matches.get_one::<PathBuf>("manifest") {
        Some(path) => config::load_manifest(path),
        None => config::discover_project_root(),
    };
    result.unwrap_or_else(|e| {
        eprintln!("error: {e}");
        process::exit(1);
    })
}

pub fn read_file(path: &PathBuf) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("error reading {}: {e}", path.display());
        process::exit(1);
    })
}

pub fn resolve_test_paths(
    matches: &clap::ArgMatches,
    project_root: &std::path::Path,
) -> Vec<ModulePath> {
    let relux_dir = project_root.join(config::RELUX_DIR);
    let paths: Option<Vec<PathBuf>> = matches
        .get_many::<PathBuf>("paths")
        .map(|p| p.cloned().collect());

    match paths {
        Some(files) => files
            .iter()
            .filter_map(|f| {
                let abs = if f.is_relative() {
                    std::env::current_dir().ok()?.join(f)
                } else {
                    f.clone()
                };
                let rel = abs.strip_prefix(&relux_dir).ok()?;
                let without_ext = rel.with_extension("");
                let mod_path = without_ext.to_string_lossy().replace('\\', "/");
                Some(ModulePath(mod_path))
            })
            .collect(),
        None => {
            let test_dir = config::tests_dir(project_root);
            discover_test_modules(&test_dir, &relux_dir)
        }
    }
}

pub fn build_source_loader(
    project_root: &std::path::Path,
) -> Box<dyn relux_resolver::SourceLoader> {
    let relux_dir = project_root.join(config::RELUX_DIR);
    Box::new(FsSourceLoader::new(relux_dir, vec![]))
}
