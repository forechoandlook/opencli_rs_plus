pub mod adapter_manager;
pub mod autostart;
pub mod client;
pub mod extension_api;
pub mod plugin;
pub mod socket;

use anyhow::Result;
use std::path::PathBuf;

pub fn default_addr() -> String {
    "127.0.0.1:10008".to_string()
}

pub fn default_log_path() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".opencli-rs").join("daemon.log"))
        .unwrap_or_else(|| PathBuf::from("daemon.log"))
}

pub fn default_pid_path() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".opencli-rs").join("daemon.pid"))
        .unwrap_or_else(|| PathBuf::from("daemon.pid"))
}

/// Start the opencli daemon (adapter/plugin + extension API). Blocks until Ctrl-C.
pub async fn run_daemon(addr: String) -> Result<()> {
    use std::sync::Arc;
    use tokio::signal;
    use tracing::info;

    let pid_path = default_pid_path();
    if let Some(parent) = pid_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&pid_path, std::process::id().to_string());

    let adapter_manager = Arc::new(adapter_manager::AdapterManager::new().await?);
    let plugin_manager = adapter_manager.plugin_manager();
    let socket_state = Arc::new(socket::SocketState {
        adapter_manager,
        plugin_manager,
    });

    let extension_api_addr = std::env::var("OPENCLI_EXTENSION_API_ADDR")
        .unwrap_or_else(|_| extension_api::DEFAULT_EXTENSION_API_ADDR.to_string());
    let extension_api_handle =
        extension_api::start(&extension_api_addr, Arc::clone(&socket_state)).await?;

    let addr_clone = addr.clone();
    let socket_handle = tokio::spawn(async move {
        if let Err(e) = socket::serve(&addr_clone, socket_state).await {
            tracing::error!(error = %e, "Socket server error");
        }
    });

    info!(addr = %addr, extension_api_addr = %extension_api_addr, "opencli daemon started");
    signal::ctrl_c().await?;
    info!("Shutting down opencli daemon");
    socket_handle.abort();
    extension_api_handle.abort();
    let _ = std::fs::remove_file(&pid_path);
    Ok(())
}

/// Run the daemon client (adapter/plugin/status commands).
pub fn run_client() -> Result<()> {
    client::run()
}
