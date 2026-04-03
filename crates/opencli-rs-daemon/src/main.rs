mod adapter_manager;
mod index;
mod issues;
mod scheduler;
mod socket;
mod store;
mod tools;

use adapter_manager::AdapterManager;
use anyhow::Result;
use clap::Parser;
use issues::{default_issues_db_path, IssueStore};
use scheduler::Scheduler;
use socket::{serve, SocketState};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::signal;
use tracing::{error, info, Level};
use tracing_subscriber::FmtSubscriber;

use crate::store::JobStore;

fn default_db_path() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".opencli-rs").join("jobs.db"))
        .unwrap_or_else(|| PathBuf::from("jobs.db"))
}

fn default_addr() -> String {
    "127.0.0.1:10008".to_string()
}

#[derive(Parser)]
#[command(name = "opencli-daemon", about = "OpenCLI scheduler daemon")]
struct Cli {
    /// Polling interval in seconds
    #[arg(long, default_value = "10")]
    poll_interval: u64,
    /// Database path (default: ~/.opencli-rs/jobs.db)
    #[arg(long)]
    db: Option<PathBuf>,
    /// TCP address to listen on (default: 127.0.0.1:10008)
    #[arg(long)]
    addr: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let _subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .with_target(false)
        .compact()
        .init();

    let args = Cli::parse();
    let addr = args.addr.unwrap_or_else(default_addr);
    let db_path = args.db.unwrap_or_else(default_db_path);

    let job_store = Arc::new(JobStore::new(db_path).map_err(|e| anyhow::anyhow!("{}", e))?);
    let adapter_manager = Arc::new(AdapterManager::new().await?);
    let issue_store = Arc::new(IssueStore::new(default_issues_db_path())?);
    let scheduler = Arc::new(Scheduler::new(
        Arc::clone(&job_store),
        Arc::clone(&adapter_manager),
        args.poll_interval,
    ));

    let sched = Arc::clone(&scheduler);
    let sched_handle = tokio::spawn(async move {
        sched.run_loop().await;
    });

    let socket_state = Arc::new(SocketState {
        adapter_manager,
        scheduler,
        job_store,
        issue_store,
    });

    let addr_clone = addr.clone();
    let socket_handle = tokio::spawn(async move {
        if let Err(e) = serve(&addr_clone, socket_state).await {
            error!(error = %e, "Socket server error");
        }
    });

    info!(addr = %addr, poll_interval = args.poll_interval, "Daemon started");

    signal::ctrl_c().await?;
    info!("Shutting down daemon");
    sched_handle.abort();
    socket_handle.abort();
    Ok(())
}
