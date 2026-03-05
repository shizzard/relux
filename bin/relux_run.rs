use std::{env, fs, path::PathBuf, process};

use chrono::Utc;
use relux::dsl::report::print_diagnostics;
use relux::dsl::resolver::resolve;
use relux::runtime::result::Reporter;
use relux::runtime::{RunContext, Runtime};

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: relux_run <file.relux> [file2.relux ...]");
        eprintln!("  resolves from the current directory as project root");
        process::exit(1);
    }

    let project_root = env::current_dir().unwrap_or_else(|e| {
        eprintln!("error: cannot determine current directory: {e}");
        process::exit(1);
    });
    let roots: Vec<PathBuf> = args.iter().map(PathBuf::from).collect();

    let run_context = create_run_context(&project_root).unwrap_or_else(|e| {
        eprintln!("error: cannot create relux run directories: {e}");
        process::exit(1);
    });

    let (plans, source_map, diagnostics) = resolve(&roots, &project_root);
    if !diagnostics.is_empty() {
        print_diagnostics(&diagnostics, &source_map);
        process::exit(1);
    }

    let runtime = Runtime::new(source_map, run_context);
    let results = runtime.run(plans).await;
    Reporter::print(&results, runtime.source_map());

    let failed = results
        .iter()
        .any(|r| matches!(r.outcome, relux::runtime::result::Outcome::Fail(_)));
    if failed {
        process::exit(1);
    }
}

fn create_run_context(project_root: &std::path::Path) -> Result<RunContext, std::io::Error> {
    let out_root = project_root.join("relux-out");
    fs::create_dir_all(&out_root)?;

    let timestamp = Utc::now().format("%Y-%m-%d-%H-%M-%S").to_string();
    for _ in 0..32 {
        let run_id = generate_run_id();
        let run_dir = out_root.join(format!("run-{timestamp}-{run_id}"));
        let artifacts_dir = run_dir.join("artifacts");

        match fs::create_dir(&run_dir) {
            Ok(()) => {
                fs::create_dir_all(&artifacts_dir)?;
                return Ok(RunContext {
                    run_id,
                    artifacts_dir,
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                continue;
            }
            Err(e) => return Err(e),
        }
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "failed to generate a unique run directory",
    ))
}

fn generate_run_id() -> String {
    let bytes: [u8; 16] = rand::random();
    bs58::encode(bytes).into_string().chars().take(10).collect()
}
