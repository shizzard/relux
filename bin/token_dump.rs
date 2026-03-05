use std::{env, fs, process};

use relux::dsl::lexer::lex;

fn main() {
    let path = match env::args().nth(1) {
        Some(p) => p,
        None => {
            eprintln!("usage: token_dump <file.relux>");
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

    for spanned in lex(&source) {
        print!("{:?} ", spanned.node);
    }
}
