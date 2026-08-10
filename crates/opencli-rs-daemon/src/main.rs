use clap::Parser;
use chrono::Local;
use opencli_rs_daemon::{default_addr, run_client, run_daemon};
use tracing::Level;
use tracing_subscriber::{
    fmt::time::FormatTime,
    EnvFilter, FmtSubscriber,
};

/// Daemon subcommand args
#[derive(Parser)]
struct DaemonArgs {
    #[arg(long)]
    addr: Option<String>,
}

// Subcommands that belong to the daemon client (top-level aliases collapsed into daemon/*)
const CLIENT_SUBCMDS: &[&str] = &["adapter", "plugin", "kv"];

#[tokio::main]
async fn main() {
    // Init tracing once for the unified binary.
    FmtSubscriber::builder()
        .with_env_filter(EnvFilter::try_from_env("RUST_LOG").unwrap_or_else(|_| {
            if std::env::var("OPENCLI_VERBOSE").is_ok() {
                EnvFilter::new("info")
            } else {
                EnvFilter::new("off")
            }
        }))
        .with_max_level(Level::INFO)
        .with_target(false)
        .with_timer(log_timer())
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
    let daemon_second = raw
        .iter()
        .skip(2)
        .find(|a| !a.starts_with('-'))
        .map(|s| s.as_str());
    const DAEMON_MGMT_SUBCMDS: &[&str] = &[
        "start", "stop", "restart", "status", "logs", "config", "autostart",
    ];

    match subcmd {
        // ── Daemon process management (background start/stop/logs/autostart) ──
        Some("daemon") if daemon_second.is_some_and(|s| DAEMON_MGMT_SUBCMDS.contains(&s)) => {
            if let Err(e) = run_client() {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }

        // ── Daemon (foreground) ────────────────────────────────────
        Some("daemon") => {
            // Strip "daemon" from args before passing to clap
            let daemon_args: Vec<String> = std::iter::once(raw[0].clone())
                .chain(raw.iter().skip(2).cloned())
                .collect();
            let args = DaemonArgs::parse_from(daemon_args);
            let addr = args.addr.unwrap_or_else(default_addr);
            if let Err(e) = run_daemon(addr).await {
                eprintln!("Daemon error: {}", e);
                std::process::exit(1);
            }
        }

        // ── Daemon client ───────────────────────────────────────────────
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

fn log_timer() -> impl FormatTime {
    match std::env::var("OPENCLI_LOG_TIME")
        .ok()
        .as_deref()
        .map(|s| s.to_ascii_lowercase())
        .as_deref()
    {
        Some("none") => LogTimer::None,
        Some("second") => LogTimer::Second,
        Some("millisecond") | Some("ms") => LogTimer::Millisecond,
        Some("minute") | None => LogTimer::Minute,
        Some(_) => LogTimer::Minute,
    }
}

enum LogTimer {
    None,
    Minute,
    Second,
    Millisecond,
}

impl FormatTime for LogTimer {
    fn format_time(&self, writer: &mut tracing_subscriber::fmt::format::Writer<'_>) -> std::fmt::Result {
        match self {
            LogTimer::None => Ok(()),
            LogTimer::Minute => write!(writer, "{}", Local::now().format("%Y-%m-%d %H:%M")),
            LogTimer::Second => write!(writer, "{}", Local::now().format("%Y-%m-%d %H:%M:%S")),
            LogTimer::Millisecond => write!(
                writer,
                "{}",
                Local::now().format("%Y-%m-%d %H:%M:%S%.3f")
            ),
        }
    }
}
