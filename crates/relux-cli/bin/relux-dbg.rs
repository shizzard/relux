use std::process;

fn main() {
    let mut terminal = relux::dbg::tui::init_terminal().unwrap_or_else(|e| {
        eprintln!("error: failed to initialize terminal: {e}");
        process::exit(1);
    });

    let result = relux::dbg::tui::App::new().run(&mut terminal);

    relux::dbg::tui::restore_terminal();

    if let Err(e) = result {
        eprintln!("error: {e}");
        process::exit(1);
    }
}
