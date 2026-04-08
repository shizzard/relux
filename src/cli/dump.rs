use std::path::PathBuf;
use std::process;
use std::sync::Arc;

use crate::core::config;
use crate::dsl::lexer::lex;
use crate::dsl::lexer::normalize;
use crate::dsl::parser::parse;
use crate::dsl::resolver::ir::Plan;
use crate::dsl::resolver::resolve;
use crate::pure::Env;
use crate::pure::LayeredEnv;

use super::build_source_loader;
use super::read_file;

pub fn cmd_dump_tokens(matches: &clap::ArgMatches) {
    let path: &PathBuf = matches.get_one("file").unwrap();
    let source = read_file(path);
    let normalized = normalize(&source);
    for spanned in lex(&normalized) {
        print!("{:?} ", spanned.node);
    }
    println!();
}

pub fn cmd_dump_ast(matches: &clap::ArgMatches) {
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

pub fn cmd_dump_ir(matches: &clap::ArgMatches) {
    let (project_root, _config) = config::discover_project_root().unwrap_or_else(|e| {
        eprintln!("error: {e}");
        process::exit(1);
    });

    let files: Vec<PathBuf> = matches
        .get_many::<PathBuf>("files")
        .unwrap()
        .cloned()
        .collect();

    // Convert file paths to module paths relative to relux dir
    let relux_dir = project_root.join(config::RELUX_DIR);
    let test_paths: Vec<_> = files
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
            Some(crate::diagnostics::ModulePath(mod_path))
        })
        .collect();

    let loader = build_source_loader(&project_root);
    let env = Arc::new(LayeredEnv::from(Env::capture()));
    let suite = resolve(&*loader, test_paths, env, 1.0, &project_root);

    let mut first = true;
    for plan in &suite.plans {
        if let Plan::Runnable { test, .. } = plan {
            if !first {
                println!("\n{}", "─".repeat(60));
            }
            println!("{test:#?}");
            first = false;
        }
    }
}
