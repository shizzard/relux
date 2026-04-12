use std::path::PathBuf;
use std::process;

use clap::Arg;
use clap::Command;
use clap::value_parser;
use clap_complete::engine::ArgValueCompleter;

fn main() {
    let matches = Command::new("relux-dbg")
        .about("Relux TUI debugger")
        .arg(
            Arg::new("manifest")
                .long("manifest")
                .help("Path to the suite manifest file (default: auto-discover Relux.toml)")
                .value_parser(value_parser!(PathBuf))
                .add(ArgValueCompleter::new(relux::completer::complete_manifest)),
        )
        .get_matches();

    let (project_root, _config) = relux::resolve_project(&matches);

    let mut terminal = relux::dbg::tui::init_terminal().unwrap_or_else(|e| {
        eprintln!("error: failed to initialize terminal: {e}");
        process::exit(1);
    });

    let result = relux::dbg::tui::App::new(project_root).run(&mut terminal);

    relux::dbg::tui::restore_terminal();

    if let Err(e) = result {
        eprintln!("error: {e}");
        process::exit(1);
    }
}
