mod connect;
mod error;
mod message;
mod request;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

// ─── CLI ──────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "rdp-client", about = "Relux Debug Protocol test client", version = relux_core::VERSION)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Connect to an RDP server and enter the message loop
    Connect {
        /// Server host
        #[arg(long, default_value = "127.0.0.1")]
        host: String,

        /// Server port
        #[arg(long, default_value_t = 9377)]
        port: u16,

        /// Working directory for message files (relative to cwd)
        #[arg(long)]
        dir: PathBuf,
    },

    /// Generate a JSON-RPC request file
    Request {
        /// Method name (e.g. session/init)
        method: String,

        /// JSON-RPC request id
        #[arg(long)]
        id: u64,

        /// Working directory to write the request file into
        #[arg(long)]
        dir: PathBuf,
    },
}

// ─── Entry point ──────────────────────────────────────────

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Connect { host, port, dir } => connect::cmd_connect(&host, port, &dir).await,
        Commands::Request { method, id, dir } => request::cmd_request(&method, id, &dir),
    };

    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
