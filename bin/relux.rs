use relux::cli;

#[tokio::main]
async fn main() {
    clap_complete::env::CompleteEnv::with_factory(cli::cli).complete();

    let matches = cli::cli().get_matches();

    match matches.subcommand() {
        Some(("new", sub)) => cli::new::cmd_new(sub),
        Some(("run", sub)) => cli::run::cmd_run(sub).await,
        Some(("check", sub)) => cli::check::cmd_check(sub),
        Some(("history", sub)) => cli::history::cmd_history(sub),
        Some(("completions", sub)) => cli::completions::cmd_completions(sub),
        Some(("dump", sub)) => match sub.subcommand() {
            Some(("tokens", sub)) => cli::dump::cmd_dump_tokens(sub),
            Some(("ast", sub)) => cli::dump::cmd_dump_ast(sub),
            Some(("ir", sub)) => cli::dump::cmd_dump_ir(sub),
            _ => unreachable!(),
        },
        _ => unreachable!(),
    }
}
