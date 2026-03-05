use std::{env, fs, process};

use relux::dsl::parser::parse;

fn main() {
    let path = match env::args().nth(1) {
        Some(p) => p,
        None => {
            eprintln!("usage: ast_dump <file.relux>");
            process::exit(1);
        }
    };

    let source = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error reading {path}: {e}");
            process::exit(1);
        }
    };

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
