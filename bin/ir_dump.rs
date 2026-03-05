use std::{env, path::PathBuf, process};

use relux::dsl::report::print_diagnostics;
use relux::dsl::resolver::resolve;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: ir_dump <file.relux> [file2.relux ...]");
        eprintln!("  resolves from the current directory as project root");
        process::exit(1);
    }

    let project_root = env::current_dir().unwrap_or_else(|e| {
        eprintln!("error: cannot determine current directory: {e}");
        process::exit(1);
    });

    let roots: Vec<PathBuf> = args.iter().map(PathBuf::from).collect();
    let (plans, source_map, diagnostics) = resolve(&roots, &project_root);

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
