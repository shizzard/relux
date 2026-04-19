#[tokio::main]
async fn main() {
    clap_complete::env::CompleteEnv::with_factory(relux::cli).complete();

    let matches = relux::cli().get_matches();

    match matches.subcommand() {
        Some(("init", _)) => relux::init::cmd_init(),
        Some(("new", sub)) => relux::new::cmd_new(sub),
        Some(("run", sub)) => relux::run::cmd_run(sub).await,
        Some(("check", sub)) => relux::check::cmd_check(sub),
        Some(("history", sub)) => relux::history::cmd_history(sub),
        Some(("completions", sub)) => relux::completions::cmd_completions(sub),
        Some(("dump", sub)) => match sub.subcommand() {
            Some(("tokens", sub)) => relux::dump::cmd_dump_tokens(sub),
            Some(("ast", sub)) => relux::dump::cmd_dump_ast(sub),
            Some(("ir", sub)) => relux::dump::cmd_dump_ir(sub),
            _ => unreachable!(),
        },
        _ => unreachable!(),
    }
}
