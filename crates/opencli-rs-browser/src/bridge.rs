use opencli_rs_core::{CliError, IPage};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::daemon_client::DaemonClient;
use crate::page::DaemonPage;

const PORT_RANGE_START: u16 = 19825;
const PORT_RANGE_END: u16 = 19834;
const READY_TIMEOUT: Duration = Duration::from_secs(10);
const READY_POLL_INTERVAL: Duration = Duration::from_millis(200);
const EXTENSION_INITIAL_WAIT: Duration = Duration::from_secs(5);
const EXTENSION_REMAINING_WAIT: Duration = Duration::from_secs(25);
const EXTENSION_POLL_INTERVAL: Duration = Duration::from_millis(500);
const PORT_CHECK_TIMEOUT: Duration = Duration::from_millis(100);

/// High-level bridge that manages the Daemon process and provides IPage instances.
/// The daemon runs as a detached background process with its own idle-shutdown lifecycle.
/// Supports multiple daemons on ports 19825-19834 for multi-browser scenarios.
pub struct BrowserBridge {
    port: Option<u16>,
}

impl BrowserBridge {
    pub fn new(port: u16) -> Self {
        Self { port: Some(port) }
    }

    /// Create a bridge that auto-detects daemon on available ports.
    pub fn default_port() -> Self {
        Self { port: None }
    }

    /// Connect to the daemon, starting it if necessary, and return a page.
    pub async fn connect(&mut self) -> Result<Arc<dyn IPage>, CliError> {
        // Step 1: Check Chrome is running
        if !is_chrome_running() {
            return Err(CliError::BrowserConnect {
                message: "Chrome is not running".into(),
                suggestions: vec![
                    "Please open Google Chrome with the OpenCLI extension installed".into(),
                    "The extension connects to the daemon automatically when Chrome is open".into(),
                ],
                source: None,
            });
        }

        // Step 2: Try to find an active daemon with extension already connected
        if let Some(port) = self.find_active_daemon_with_extension().await {
            debug!(port, "found active daemon with extension connected");
            self.port = Some(port);
            let client = Arc::new(DaemonClient::new(port));
            let page = DaemonPage::new(client, "default");
            return Ok(Arc::new(page));
        }

        // Step 3: No daemon with extension — find or spawn one
        let port = self.find_or_spawn_daemon().await?;
        self.port = Some(port);
        let client = Arc::new(DaemonClient::new(port));

        // Step 4: Wait up to 5s for extension to connect
        if self
            .poll_extension(&client, EXTENSION_INITIAL_WAIT, false)
            .await
        {
            let page = DaemonPage::new(client, "default");
            return Ok(Arc::new(page));
        }

        // Step 5: Extension not connected — try to wake up Chrome
        info!("Extension not connected after 5s, attempting to wake up Chrome");
        eprintln!("Waking up Chrome extension...");
        wake_chrome();

        // Step 6: Wait remaining 25s with progress
        if self
            .poll_extension(&client, EXTENSION_REMAINING_WAIT, true)
            .await
        {
            let page = DaemonPage::new(client, "default");
            return Ok(Arc::new(page));
        }

        warn!("Chrome extension is not connected to the daemon");
        Err(CliError::BrowserConnect {
            message: "Chrome extension not connected".into(),
            suggestions: vec![
                "Make sure the OpenCLI Chrome extension is installed and enabled".into(),
                "Try opening a new Chrome window manually".into(),
                format!("The daemon is listening on port {}", port),
            ],
            source: None,
        })
    }

    /// Scan ports 19825-19834 and return the first port where a daemon is running with extension connected.
    async fn find_active_daemon_with_extension(&self) -> Option<u16> {
        for port in PORT_RANGE_START..=PORT_RANGE_END {
            let client = DaemonClient::new(port);
            if client.is_running().await && client.is_extension_connected().await {
                return Some(port);
            }
        }
        None
    }

    /// Find an existing daemon or spawn a new one.
    /// Returns the port on which a daemon is running.
    async fn find_or_spawn_daemon(&mut self) -> Result<u16, CliError> {
        // First, check if user specified a port
        if let Some(port) = self.port {
            let client = DaemonClient::new(port);
            if client.is_running().await {
                debug!(port, "daemon already running on specified port");
                return Ok(port);
            } else {
                info!(port, "spawning daemon on specified port");
                self.spawn_daemon(port).await?;
                self.wait_for_ready(&DaemonClient::new(port)).await?;
                return Ok(port);
            }
        }

        // Otherwise, scan for any running daemon
        for port in PORT_RANGE_START..=PORT_RANGE_END {
            let client = DaemonClient::new(port);
            if client.is_running().await {
                debug!(port, "found existing daemon");
                return Ok(port);
            }
        }

        // No daemon found — spawn a new one on the first free port
        for port in PORT_RANGE_START..=PORT_RANGE_END {
            if !is_port_in_use(port) {
                info!(port, "spawning daemon on free port");
                self.spawn_daemon(port).await?;
                self.wait_for_ready(&DaemonClient::new(port)).await?;
                return Ok(port);
            }
        }

        Err(CliError::browser_connect(
            "All daemon ports (19825-19834) are in use",
        ))
    }

    /// Spawn the daemon as a child process using --daemon flag on the current binary.
    async fn spawn_daemon(&self, port: u16) -> Result<(), CliError> {
        let exe = std::env::current_exe().map_err(|e| {
            CliError::browser_connect(format!("Cannot determine current executable: {e}"))
        })?;

        let child = tokio::process::Command::new(exe)
            .arg("--daemon")
            .arg("--port")
            .arg(port.to_string())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| CliError::browser_connect(format!("Failed to spawn daemon: {e}")))?;

        info!(port = port, pid = ?child.id(), "daemon process spawned (detached)");
        std::mem::forget(child);
        Ok(())
    }

    /// Poll for extension connection within the given duration.
    /// Returns true if connected, false if timed out.
    async fn poll_extension(
        &self,
        client: &DaemonClient,
        timeout: Duration,
        show_progress: bool,
    ) -> bool {
        let start = tokio::time::Instant::now();
        let deadline = start + timeout;
        let mut printed = false;

        while tokio::time::Instant::now() < deadline {
            if client.is_extension_connected().await {
                if printed {
                    eprintln!();
                }
                info!("Chrome extension connected");
                return true;
            }

            if show_progress {
                let elapsed = start.elapsed().as_secs();
                if elapsed >= 1 && !printed {
                    eprint!("Waiting for Chrome extension to connect");
                    printed = true;
                } else if printed && elapsed % 3 == 0 {
                    eprint!(".");
                }
            }

            tokio::time::sleep(EXTENSION_POLL_INTERVAL).await;
        }

        if printed {
            eprintln!();
        }
        false
    }

    /// Wait for the daemon to become ready by polling /health.
    async fn wait_for_ready(&self, client: &DaemonClient) -> Result<(), CliError> {
        let deadline = tokio::time::Instant::now() + READY_TIMEOUT;

        while tokio::time::Instant::now() < deadline {
            if client.is_running().await {
                info!("daemon is ready");
                return Ok(());
            }
            tokio::time::sleep(READY_POLL_INTERVAL).await;
        }

        Err(CliError::timeout(format!(
            "Daemon did not become ready within {}s",
            READY_TIMEOUT.as_secs()
        )))
    }
}

/// Check if a port is in use by attempting a TCP connection.
fn is_port_in_use(port: u16) -> bool {
    let addr = format!("127.0.0.1:{}", port);
    match TcpStream::connect_timeout(&addr.parse().unwrap(), PORT_CHECK_TIMEOUT) {
        Ok(_) => true,
        Err(_) => false,
    }
}

/// Check if Chrome/Chromium is running as a process.
fn is_chrome_running() -> bool {
    if cfg!(target_os = "macos") {
        // macOS: check for "Google Chrome" process
        std::process::Command::new("pgrep")
            .args(["-x", "Google Chrome"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    } else if cfg!(target_os = "windows") {
        // Windows: check for chrome.exe
        std::process::Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq chrome.exe", "/NH"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("chrome.exe"))
            .unwrap_or(false)
    } else {
        // Linux: check for chrome or chromium
        std::process::Command::new("pgrep")
            .args(["-x", "chrome|chromium"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

/// Try to wake up Chrome by opening a window.
/// When Chrome is running but has no windows, the extension Service Worker is suspended.
/// Opening a window activates the Service Worker, which then reconnects to the daemon.
fn wake_chrome() {
    let result = if cfg!(target_os = "macos") {
        std::process::Command::new("open")
            .args(["-a", "Google Chrome", "about:blank"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
    } else if cfg!(target_os = "windows") {
        std::process::Command::new("cmd")
            .args(["/C", "start", "chrome", "about:blank"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
    } else {
        // Linux: try common Chrome executables
        std::process::Command::new("xdg-open")
            .arg("about:blank")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
    };

    match result {
        Ok(_) => debug!("Opened Chrome window to wake extension"),
        Err(e) => debug!("Failed to open Chrome window: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bridge_construction() {
        let bridge = BrowserBridge::new(19825);
        assert_eq!(bridge.port, Some(19825));
    }

    #[test]
    fn test_bridge_default_port() {
        let bridge = BrowserBridge::default_port();
        assert_eq!(bridge.port, None);
    }

    #[test]
    fn test_port_range_constants() {
        // Verify port range is reasonable (10 ports)
        assert_eq!(PORT_RANGE_START, 19825);
        assert_eq!(PORT_RANGE_END, 19834);
        assert_eq!(PORT_RANGE_END - PORT_RANGE_START + 1, 10);
    }

    #[test]
    fn test_port_in_use_detection() {
        // Localhost on a very high port is unlikely to be in use
        let unlikely_port = 59999;
        assert!(
            !is_port_in_use(unlikely_port),
            "Unlikely port should not be in use"
        );

        // Localhost on port 1 should fail (requires privilege)
        // We skip this test as it requires elevated privileges
        // assert!(is_port_in_use(1));
    }
}
