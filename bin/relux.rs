use std::path::PathBuf;
use std::sync::Arc;
use std::{fs, process};

use clap::{Arg, ArgAction, Command, value_parser};

use relux::config::{self, ReluxConfig};
use relux::dsl::lexer::{lex, normalize};
use relux::dsl::parser::parse;
use relux::dsl::resolver::ir::NewPlan;
use relux::dsl::resolver::{FsSourceLoader, discover_test_modules, resolve};
use relux::runtime::history::{HistoryCommand, OutputFormat, run_history};
use relux::stack::Env;

fn cli() -> Command {
    Command::new("relux")
        .about("Relux test runner")
        .subcommand_required(true)
        .subcommand(
            Command::new("new")
                .about("Scaffold a new Relux project, test, or effect")
                .arg(
                    Arg::new("test")
                        .long("test")
                        .help("Create a test module (e.g. foo/bar/baz)")
                        .value_name("MODULE_PATH")
                        .conflicts_with("effect"),
                )
                .arg(
                    Arg::new("effect")
                        .long("effect")
                        .help("Create an effect module (e.g. foo/bar/baz)")
                        .value_name("MODULE_PATH")
                        .conflicts_with("test"),
                ),
        )
        .subcommand(
            Command::new("run")
                .about("Run tests")
                .arg(
                    Arg::new("paths")
                        .help("Test files or directories to run (default: relux/tests/)")
                        .num_args(0..)
                        .value_parser(value_parser!(PathBuf)),
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
                        .help("Scale tolerance (~) timeouts by this factor; assertion (@) timeouts are not scaled")
                        .value_parser(value_parser!(f64))
                        .default_value("1.0"),
                )
                .arg(
                    Arg::new("progress")
                        .long("progress")
                        .help("Real-time output verbosity")
                        .value_parser(["quiet", "basic", "verbose"])
                        .default_value("basic"),
                )
                .arg(
                    Arg::new("strategy")
                        .long("strategy")
                        .help("Run strategy: 'all' runs every test; 'fail-fast' stops at first failure")
                        .value_parser(["all", "fail-fast"])
                        .default_value("all"),
                )
                .arg(
                    Arg::new("rerun")
                        .long("rerun")
                        .help("Re-run only failed tests from the latest run")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("manifest")
                        .long("manifest")
                        .help("Path to the suite manifest file (default: auto-discover Relux.toml)")
                        .value_parser(value_parser!(PathBuf)),
                ),
        )
        .subcommand(
            Command::new("check")
                .about("Validate test files without executing")
                .arg(
                    Arg::new("paths")
                        .help("Test files or directories to check (default: relux/tests/)")
                        .num_args(0..)
                        .value_parser(value_parser!(PathBuf)),
                )
                .arg(
                    Arg::new("manifest")
                        .long("manifest")
                        .help("Path to the suite manifest file (default: auto-discover Relux.toml)")
                        .value_parser(value_parser!(PathBuf)),
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
                        .value_parser(value_parser!(PathBuf)),
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
                        .value_parser(value_parser!(PathBuf)),
                ),
        )
        .subcommand(
            Command::new("dump")
                .about("Introspection tools")
                .subcommand_required(true)
                .subcommand(
                    Command::new("tokens")
                        .about("Dump lexer tokens")
                        .arg(
                            Arg::new("file")
                                .help("File to tokenize")
                                .required(true)
                                .value_parser(value_parser!(PathBuf)),
                        ),
                )
                .subcommand(
                    Command::new("ast")
                        .about("Dump parsed AST")
                        .arg(
                            Arg::new("file")
                                .help("File to parse")
                                .required(true)
                                .value_parser(value_parser!(PathBuf)),
                        ),
                )
                .subcommand(
                    Command::new("ir")
                        .about("Dump resolved IR")
                        .arg(
                            Arg::new("files")
                                .help("Files to resolve")
                                .required(true)
                                .num_args(1..)
                                .value_parser(value_parser!(PathBuf)),
                        ),
                ),
        )
}

#[tokio::main]
async fn main() {
    let matches = cli().get_matches();

    match matches.subcommand() {
        Some(("new", sub)) => cmd_new(sub),
        Some(("run", sub)) => cmd_run(sub).await,
        Some(("check", sub)) => cmd_check(sub),
        Some(("history", sub)) => cmd_history(sub),
        Some(("dump", sub)) => match sub.subcommand() {
            Some(("tokens", sub)) => cmd_dump_tokens(sub),
            Some(("ast", sub)) => cmd_dump_ast(sub),
            Some(("ir", sub)) => cmd_dump_ir(sub),
            _ => unreachable!(),
        },
        _ => unreachable!(),
    }
}

fn cmd_new(matches: &clap::ArgMatches) {
    if let Some(module_path) = matches.get_one::<String>("test") {
        cmd_new_module(module_path, ModuleKind::Test);
    } else if let Some(module_path) = matches.get_one::<String>("effect") {
        cmd_new_module(module_path, ModuleKind::Effect);
    } else {
        cmd_new_project();
    }
}

fn cmd_new_project() {
    let cwd = std::env::current_dir().unwrap_or_else(|e| {
        eprintln!("error: cannot determine current directory: {e}");
        process::exit(1);
    });

    let toml_path = cwd.join(config::CONFIG_FILE);
    if toml_path.exists() {
        eprintln!("error: {} already exists", config::CONFIG_FILE);
        process::exit(1);
    }

    let relux_dir = cwd.join(config::RELUX_DIR);
    let tests_dir = relux_dir.join(config::TESTS_DIR);
    let lib_dir = relux_dir.join(config::LIB_DIR);
    let gitignore_path = relux_dir.join(".gitignore");

    fs::create_dir_all(&tests_dir).unwrap_or_else(|e| {
        eprintln!("error: cannot create tests directory: {e}");
        process::exit(1);
    });
    fs::create_dir_all(&lib_dir).unwrap_or_else(|e| {
        eprintln!("error: cannot create lib directory: {e}");
        process::exit(1);
    });

    let toml_content = format!(
        r#"# name = "my-test-suite"

# [shell]
# command = "{command}"
# prompt = "{prompt}"

# [timeout]
# match = "5s"
# test = "5m"
# suite = "30m"
"#,
        command = config::DEFAULT_SHELL_COMMAND,
        prompt = config::DEFAULT_SHELL_PROMPT,
    );

    fs::write(&toml_path, toml_content).unwrap_or_else(|e| {
        eprintln!("error: cannot write {}: {e}", config::CONFIG_FILE);
        process::exit(1);
    });

    fs::write(&gitignore_path, "out/\n").unwrap_or_else(|e| {
        eprintln!("error: cannot write .gitignore: {e}");
        process::exit(1);
    });

    eprintln!("Created {}", config::CONFIG_FILE);
    eprintln!("Created {}/{}/", config::RELUX_DIR, config::TESTS_DIR);
    eprintln!("Created {}/{}/", config::RELUX_DIR, config::LIB_DIR);
    eprintln!("Created {}/.gitignore", config::RELUX_DIR);
}

enum ModuleKind {
    Test,
    Effect,
}

fn validate_module_path(raw: &str) -> Result<String, String> {
    let normalized = raw.strip_suffix(".relux").unwrap_or(raw);
    if normalized.is_empty() {
        return Err("module path cannot be empty".to_string());
    }
    for segment in normalized.split('/') {
        if segment.is_empty() {
            return Err("module path contains empty segment".to_string());
        }
        if !segment.chars().next().unwrap().is_ascii_lowercase() && !segment.starts_with('_') {
            return Err(format!(
                "segment `{segment}` must start with a lowercase letter or underscore"
            ));
        }
        if !segment
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            return Err(format!(
                "segment `{segment}` must contain only lowercase letters, digits, and underscores"
            ));
        }
    }
    Ok(normalized.to_string())
}

fn cmd_new_module(raw_path: &str, kind: ModuleKind) {
    let module_path = validate_module_path(raw_path).unwrap_or_else(|e| {
        eprintln!("error: invalid module path: {e}");
        process::exit(1);
    });

    let (project_root, _config) = config::discover_project_root().unwrap_or_else(|e| {
        eprintln!("error: {e}");
        process::exit(1);
    });

    let base_dir = match kind {
        ModuleKind::Test => config::tests_dir(&project_root),
        ModuleKind::Effect => config::lib_dir(&project_root),
    };

    let file_path = base_dir.join(&module_path).with_extension("relux");
    if file_path.exists() {
        eprintln!("error: {} already exists", file_path.display());
        process::exit(1);
    }

    if let Some(parent) = file_path.parent() {
        fs::create_dir_all(parent).unwrap_or_else(|e| {
            eprintln!("error: cannot create directory {}: {e}", parent.display());
            process::exit(1);
        });
    }

    let last_segment = module_path.rsplit('/').next().unwrap();

    let content = match kind {
        ModuleKind::Test => format!(
            r#"test {name} {{
    shell myshell {{
        > echo hello-relux
        <= hello-relux
    }}
}}
"#,
            name = last_segment.replace('_', " "),
        ),
        ModuleKind::Effect => format!(
            r#"effect {name} -> myshell {{
    shell myshell {{
    }}
}}
"#,
            name = capitalize_effect_name(last_segment),
        ),
    };

    fs::write(&file_path, content).unwrap_or_else(|e| {
        eprintln!("error: cannot write {}: {e}", file_path.display());
        process::exit(1);
    });

    let relative = file_path.strip_prefix(&project_root).unwrap_or(&file_path);
    eprintln!("Created {}", relative.display());
}

fn capitalize_effect_name(segment: &str) -> String {
    segment
        .split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => c.to_uppercase().to_string() + chars.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join("")
}

async fn cmd_run(_matches: &clap::ArgMatches) {
    // TODO(R004): runtime adaptation — needs runtime pipeline updated to work with new IR
    todo!("R004: runtime adaptation")
}

fn cmd_history(matches: &clap::ArgMatches) {
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

fn cmd_check(matches: &clap::ArgMatches) {
    let (project_root, _config) = resolve_project(matches);
    let test_paths = resolve_test_paths(matches, &project_root);
    let loader = build_source_loader(&project_root);
    let env = Arc::new(Env::capture());

    let suite = resolve(&*loader, test_paths, env);

    // Diagnostics are already printed inside resolve().
    // Check if any plan is Invalid → exit 1.
    let has_invalid = suite
        .plans
        .iter()
        .any(|p| matches!(p, NewPlan::Invalid { .. }));
    if has_invalid {
        process::exit(1);
    }

    eprintln!("check passed");
}

fn cmd_dump_tokens(matches: &clap::ArgMatches) {
    let path: &PathBuf = matches.get_one("file").unwrap();
    let source = read_file(path);
    let normalized = normalize(&source);
    for spanned in lex(&normalized) {
        print!("{:?} ", spanned.node);
    }
    println!();
}

fn cmd_dump_ast(matches: &clap::ArgMatches) {
    let path: &PathBuf = matches.get_one("file").unwrap();
    let source = read_file(path);
    match parse(&source) {
        Ok(module) => println!("{module:#?}"),
        Err(error) => {
            eprintln!("error: {error}");
            process::exit(1);
        }
    }
}

fn cmd_dump_ir(matches: &clap::ArgMatches) {
    let (project_root, _config) = config::discover_project_root().unwrap_or_else(|e| {
        eprintln!("error: {e}");
        process::exit(1);
    });

    let files: Vec<PathBuf> = matches
        .get_many::<PathBuf>("files")
        .unwrap()
        .cloned()
        .collect();

    // Convert file paths to module paths relative to project root
    let test_paths: Vec<_> = files
        .iter()
        .filter_map(|f| {
            let abs = if f.is_relative() {
                std::env::current_dir().ok()?.join(f)
            } else {
                f.clone()
            };
            let rel = abs.strip_prefix(&project_root).ok()?;
            let without_ext = rel.with_extension("");
            let mod_path = without_ext.to_string_lossy().replace('\\', "/");
            Some(relux::diagnostics::ModulePath(mod_path))
        })
        .collect();

    let loader = build_source_loader(&project_root);
    let env = Arc::new(Env::capture());
    let suite = resolve(&*loader, test_paths, env);

    let mut first = true;
    for plan in &suite.plans {
        if let NewPlan::Runnable { test, .. } = plan {
            if !first {
                println!("\n{}", "─".repeat(60));
            }
            println!("{test:#?}");
            first = false;
        }
    }
}

fn resolve_project(matches: &clap::ArgMatches) -> (PathBuf, ReluxConfig) {
    let result = match matches.get_one::<PathBuf>("manifest") {
        Some(path) => config::load_manifest(path),
        None => config::discover_project_root(),
    };
    result.unwrap_or_else(|e| {
        eprintln!("error: {e}");
        process::exit(1);
    })
}

fn read_file(path: &PathBuf) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("error reading {}: {e}", path.display());
        process::exit(1);
    })
}

fn resolve_test_paths(
    matches: &clap::ArgMatches,
    project_root: &std::path::Path,
) -> Vec<relux::diagnostics::ModulePath> {
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
                let rel = abs.strip_prefix(project_root).ok()?;
                let without_ext = rel.with_extension("");
                let mod_path = without_ext.to_string_lossy().replace('\\', "/");
                Some(relux::diagnostics::ModulePath(mod_path))
            })
            .collect(),
        None => {
            let test_dir = config::tests_dir(project_root);
            discover_test_modules(&test_dir, project_root)
        }
    }
}

fn build_source_loader(
    project_root: &std::path::Path,
) -> Box<dyn relux::dsl::resolver::SourceLoader> {
    let lib_dir = config::lib_dir(project_root);
    let extra = if lib_dir.is_dir() {
        vec![lib_dir]
    } else {
        vec![]
    };
    Box::new(FsSourceLoader::new(project_root.to_path_buf(), extra))
}
