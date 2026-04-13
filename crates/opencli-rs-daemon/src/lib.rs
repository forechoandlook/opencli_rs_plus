pub mod adapter_manager;
pub mod client;
pub mod index;
pub mod plugin;
pub mod scheduler;
pub mod socket;
pub mod store;
pub mod tools;

use anyhow::Result;
use std::path::PathBuf;

pub fn default_db_path() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".opencli-rs").join("jobs.db"))
        .unwrap_or_else(|| PathBuf::from("jobs.db"))
}

pub fn default_addr() -> String {
    "127.0.0.1:10008".to_string()
}

/// Start the scheduler daemon. Blocks until Ctrl-C.
pub async fn run_daemon(addr: String, db_path: Option<PathBuf>, poll_interval: u64) -> Result<()> {
    use std::sync::Arc;
    use tokio::signal;
    use tracing::info;

    let db_path = db_path.unwrap_or_else(default_db_path);
    let job_store = Arc::new(store::JobStore::new(db_path).map_err(|e| anyhow::anyhow!("{}", e))?);
    let adapter_manager = Arc::new(adapter_manager::AdapterManager::new().await?);
    let scheduler = Arc::new(scheduler::Scheduler::new(
        Arc::clone(&job_store),
        Arc::clone(&adapter_manager),
        poll_interval,
    ));

    let sched = Arc::clone(&scheduler);
    let sched_handle = tokio::spawn(async move { sched.run_loop().await });

    let plugin_manager = adapter_manager.plugin_manager();
    let socket_state = Arc::new(socket::SocketState {
        adapter_manager,
        scheduler,
        job_store,
        plugin_manager,
    });

    let addr_clone = addr.clone();
    let socket_handle = tokio::spawn(async move {
        if let Err(e) = socket::serve(&addr_clone, socket_state).await {
            tracing::error!(error = %e, "Socket server error");
        }
    });

    info!(addr = %addr, poll_interval, "Scheduler daemon started");
    signal::ctrl_c().await?;
    info!("Shutting down scheduler daemon");
    sched_handle.abort();
    socket_handle.abort();
    Ok(())
}

/// Run the scheduler client (job/adapter/plugin/status commands).
pub fn run_client() -> Result<()> {
    client::run()
}
