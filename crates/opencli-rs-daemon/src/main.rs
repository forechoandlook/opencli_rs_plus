use clap::Parser;
use opencli_rs_daemon::{default_addr, run_client, run_daemon};
use std::path::PathBuf;
use tracing::Level;
use tracing_subscriber::{EnvFilter, FmtSubscriber};

/// Scheduler daemon subcommand args
#[derive(Parser)]
struct DaemonArgs {
    #[arg(long, default_value = "10")]
    poll_interval: u64,
    #[arg(long)]
    db: Option<PathBuf>,
    #[arg(long)]
    addr: Option<String>,
}

// Subcommands that belong to the scheduler client
const CLIENT_SUBCMDS: &[&str] = &[
    "status", "stop", "restart", "job", "adapter", "plugin", "socket", "tools",
];

#[tokio::main]
async fn main() {
    // Init tracing once for the unified binary
    FmtSubscriber::builder()
        .with_env_filter(EnvFilter::try_from_env("RUST_LOG").unwrap_or_else(|_| {
            if std::env::var("OPENCLI_VERBOSE").is_ok() {
                EnvFilter::new("debug")
            } else {
                EnvFilter::new("warn")
            }
        }))
        .with_max_level(Level::INFO)
        .with_target(false)
        .compact()
        .init();

    let raw: Vec<String> = std::env::args().collect();

    // --daemon flag: browser-daemon mode (spawned by BrowserBridge internally)
    if raw.iter().any(|a| a == "--daemon") {
        opencli_rs_cli::runner::run().await;
        return;
    }

    // Peek at first non-flag argument to decide routing
    let subcmd = raw
        .iter()
        .skip(1)
        .find(|a| !a.starts_with('-'))
        .map(|s| s.as_str());

    match subcmd {
        // ── Scheduler daemon ───────────────────────────────────────────────
        Some("daemon") => {
            // Strip "daemon" from args before passing to clap
            let daemon_args: Vec<String> = std::iter::once(raw[0].clone())
                .chain(raw.iter().skip(2).cloned())
                .collect();
            let args = DaemonArgs::parse_from(daemon_args);
            let addr = args.addr.unwrap_or_else(default_addr);
            if let Err(e) = run_daemon(addr, args.db, args.poll_interval).await {
                eprintln!("Daemon error: {}", e);
                std::process::exit(1);
            }
        }

        // ── Scheduler client ───────────────────────────────────────────────
        Some(s) if CLIENT_SUBCMDS.contains(&s) => {
            if let Err(e) = run_client() {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }

        // ── Adapter execution (default) ────────────────────────────────────
        _ => {
            opencli_rs_cli::runner::run().await;
        }
    }
}
