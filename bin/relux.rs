use std::path::PathBuf;
use std::{fs, process};

use clap::{Arg, ArgAction, Command, value_parser};

use relux::config::{self, ReluxConfig};
use relux::dsl::lexer::lex;
use relux::dsl::parser::parse;
use relux::dsl::report::print_diagnostics;
use relux::dsl::resolver::resolve;
use relux::runtime::history::{HistoryCommand, OutputFormat, run_history};
use relux::runtime::html::generate_run_summary;
use relux::runtime::result::{Outcome, Reporter};
use relux::runtime::{RunContext, RunStrategy, Runtime};

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
                        .help("Scale all timeout values by this factor")
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
        if !segment.chars().next().unwrap().is_ascii_lowercase()
            && segment.chars().next().unwrap() != '_'
        {
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

async fn cmd_run(matches: &clap::ArgMatches) {
    let (project_root, mut relux_config) = resolve_project(matches);

    let multiplier: f64 = *matches.get_one("multiplier").unwrap();
    if (multiplier - 1.0).abs() > f64::EPSILON {
        config::apply_multiplier(&mut relux_config, multiplier);
    }

    let strategy = match matches.get_one::<String>("strategy").map(|s| s.as_str()) {
        Some("fail-fast") => RunStrategy::FailFast,
        _ => RunStrategy::All,
    };

    let (plans, source_map, _diagnostics) = if matches.get_flag("rerun") {
        let (plans, source_map, diagnostics) = resolve(&project_root, None);
        if !diagnostics.is_empty() {
            print_diagnostics(&diagnostics, &source_map);
            process::exit(1);
        }

        let out_root = config::out_dir(&project_root);
        let latest = find_latest_run(&out_root);
        let summary = relux::runtime::run_summary::read_run_summary(&latest)
            .unwrap_or_else(|e| {
                eprintln!("error: {e}");
                process::exit(1);
            });
        let failed_ids = relux::runtime::run_summary::failed_test_ids(&summary);
        if failed_ids.is_empty() {
            eprintln!("no failed tests in the latest run");
            process::exit(0);
        }

        let filtered: Vec<_> = plans
            .into_iter()
            .filter(|plan| {
                let tp = relux::runtime::compute_test_path(&source_map, &project_root, plan);
                let tn = &plan.test.name.node;
                failed_ids.iter().any(|&(p, n)| p == tp && n == tn)
            })
            .collect();

        if filtered.is_empty() {
            eprintln!("no matching test plans found for previously failed tests");
            process::exit(0);
        }

        eprintln!("re-running {} failed test(s)", filtered.len());
        (filtered, source_map, vec![])
    } else {
        let paths: Option<Vec<PathBuf>> = matches
            .get_many::<PathBuf>("paths")
            .map(|p| p.cloned().collect());
        let (plans, source_map, diagnostics) = resolve(&project_root, paths.as_deref());
        if !diagnostics.is_empty() {
            print_diagnostics(&diagnostics, &source_map);
            process::exit(1);
        }
        (plans, source_map, diagnostics)
    };

    if plans.is_empty() {
        eprintln!("no tests found");
        process::exit(0);
    }

    let run_context = create_run_context(&project_root, &relux_config, strategy);
    let run_id = run_context.run_id.clone();
    let runtime = Runtime::new(source_map, run_context);
    let results = runtime.run(plans).await;
    Reporter::print(&results, runtime.source_map());

    let suite_name = relux_config.name.as_deref().unwrap_or("relux");
    if matches.get_flag("tap") {
        relux::runtime::tap::generate_tap(
            runtime.run_dir(),
            suite_name,
            &results,
            runtime.source_map(),
        );
    }
    if matches.get_flag("junit") {
        relux::runtime::junit::generate_junit(
            runtime.run_dir(),
            suite_name,
            &results,
            runtime.source_map(),
        );
    }

    generate_run_summary(runtime.run_dir(), &results);

    let total_duration: std::time::Duration = results.iter().map(|r| r.duration).sum();
    relux::runtime::run_summary::write_run_summary(
        runtime.run_dir(),
        &run_id,
        &results,
        total_duration,
    );

    let failed = results
        .iter()
        .any(|r| matches!(r.outcome, Outcome::Fail(_)));
    if failed {
        process::exit(1);
    }
}

fn find_latest_run(out_root: &std::path::Path) -> PathBuf {
    let latest = out_root.join("latest");
    if latest.exists() {
        return latest;
    }

    // Fallback: find the most recent run-* directory by name (timestamps sort lexicographically)
    let mut run_dirs: Vec<_> = fs::read_dir(out_root)
        .unwrap_or_else(|e| {
            eprintln!("error: cannot read output directory {}: {e}", out_root.display());
            process::exit(1);
        })
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("run-") && entry.file_type().ok()?.is_dir() {
                Some(entry.path())
            } else {
                None
            }
        })
        .collect();

    run_dirs.sort();
    run_dirs.pop().unwrap_or_else(|| {
        eprintln!("error: no previous runs found in {}", out_root.display());
        process::exit(1);
    })
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

    let paths: Option<Vec<PathBuf>> = matches
        .get_many::<PathBuf>("paths")
        .map(|p| p.cloned().collect());
    let (_plans, source_map, diagnostics) = resolve(&project_root, paths.as_deref());
    if !diagnostics.is_empty() {
        print_diagnostics(&diagnostics, &source_map);
        process::exit(1);
    }

    eprintln!("check passed");
}

fn cmd_dump_tokens(matches: &clap::ArgMatches) {
    let path: &PathBuf = matches.get_one("file").unwrap();
    let source = read_file(path);
    for spanned in lex(&source) {
        print!("{:?} ", spanned.node);
    }
    println!();
}

fn cmd_dump_ast(matches: &clap::ArgMatches) {
    let path: &PathBuf = matches.get_one("file").unwrap();
    let source = read_file(path);
    let (module, errors) = parse(&source);

    if let Some(module) = module {
        println!("{module:#?}");
    }

    if !errors.is_empty() {
        eprintln!("\n--- errors ---");
        for e in &errors {
            eprintln!("  {e}");
        }
        process::exit(1);
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

    let (plans, source_map, diagnostics) = resolve(&project_root, Some(&files));

    for (i, plan) in plans.iter().enumerate() {
        if i > 0 {
            println!("\n{}", "─".repeat(60));
        }
        println!("{plan:#?}");
    }

    if !diagnostics.is_empty() {
        print_diagnostics(&diagnostics, &source_map);
        process::exit(1);
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

fn create_run_context(
    project_root: &std::path::Path,
    config: &ReluxConfig,
    strategy: RunStrategy,
) -> RunContext {
    let out_root = config::out_dir(project_root);
    fs::create_dir_all(&out_root).unwrap_or_else(|e| {
        eprintln!("error: cannot create output directory: {e}");
        process::exit(1);
    });

    let timestamp = chrono::Utc::now().format("%Y-%m-%d-%H-%M-%S").to_string();

    for _ in 0..32 {
        let run_id = generate_run_id();
        let run_dir = out_root.join(format!("run-{timestamp}-{run_id}"));
        let artifacts_dir = run_dir.join("artifacts");

        match fs::create_dir(&run_dir) {
            Ok(()) => {
                fs::create_dir_all(&artifacts_dir).unwrap_or_else(|e| {
                    eprintln!("error: cannot create artifacts directory: {e}");
                    process::exit(1);
                });

                let latest = out_root.join("latest");
                let _ = fs::remove_file(&latest);
                #[cfg(unix)]
                {
                    let _ = std::os::unix::fs::symlink(&run_dir, &latest);
                }

                return RunContext {
                    run_id,
                    run_dir,
                    artifacts_dir,
                    project_root: project_root.to_path_buf(),
                    shell_command: config.shell.command.clone(),
                    shell_prompt: config.shell.prompt.clone(),
                    default_timeout: config.timeout.match_timeout,
                    test_timeout: config.timeout.test,
                    suite_timeout: config.timeout.suite,
                    strategy,
                };
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                eprintln!("error: cannot create run directory: {e}");
                process::exit(1);
            }
        }
    }

    eprintln!("error: failed to generate a unique run directory");
    process::exit(1);
}

fn generate_run_id() -> String {
    let bytes: [u8; 16] = rand::random();
    bs58::encode(bytes).into_string().chars().take(10).collect()
}
