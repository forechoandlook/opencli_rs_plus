use colored::Colorize;
use opencli_rs_browser::DaemonClient;

const DAEMON_PORT_START: u16 = 19825;
const DAEMON_PORT_END: u16 = 19834;

fn is_binary_installed(binary: &str) -> bool {
    let cmd = if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    };
    std::process::Command::new(cmd)
        .arg(binary)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn daemon_ports_to_check() -> Vec<u16> {
    if let Some(port) = std::env::var("OPENCLI_DAEMON_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
    {
        return vec![port];
    }

    (DAEMON_PORT_START..=DAEMON_PORT_END).collect()
}

async fn find_daemon() -> Option<(u16, bool)> {
    let mut first_reachable = None;
    for port in daemon_ports_to_check() {
        let client = DaemonClient::new(port);
        if client.is_running().await {
            let extension_connected = client.is_extension_connected().await;
            if extension_connected {
                return Some((port, true));
            }
            // Keep looking: another bridge daemon may own the connected extension.
            // If none does, report the first reachable daemon for useful diagnostics.
            first_reachable.get_or_insert((port, false));
        }
    }
    first_reachable
}

pub async fn run_doctor() {
    println!("{}", "opencli-rs diagnostics".bold());
    println!();

    // 1. Check Chrome/Chromium installed
    let chrome = if cfg!(target_os = "macos") {
        is_binary_installed("google-chrome")
            || is_binary_installed("chromium")
            || std::path::Path::new("/Applications/Google Chrome.app").exists()
    } else if cfg!(target_os = "windows") {
        std::path::Path::new(r"C:\Program Files\Google\Chrome\Application\chrome.exe").exists()
            || std::path::Path::new(r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe")
                .exists()
            || is_binary_installed("chrome")
    } else {
        // Linux
        is_binary_installed("google-chrome")
            || is_binary_installed("google-chrome-stable")
            || is_binary_installed("chromium")
            || is_binary_installed("chromium-browser")
    };
    print_check("Chrome/Chromium", chrome);

    // 2. Check daemon reachable
    let daemon = find_daemon().await;
    print_check("Daemon running", daemon.is_some());

    // 3. The bridge can select any port in its supported range. Match that
    // discovery behaviour instead of incorrectly checking 19825 only.
    print_check(
        "Chrome extension connected",
        daemon.map(|(_, connected)| connected).unwrap_or(false),
    );

    // 4. Check CDP endpoint
    let cdp = std::env::var("OPENCLI_CDP_ENDPOINT").ok();
    if let Some(endpoint) = cdp {
        println!();
        println!("CDP endpoint: {}", endpoint);
    }

    // 5. Print adapter stats
    println!();
    println!("{}", "Adapter stats:".bold());
    // Will be filled in by main.rs passing registry info
}

fn print_check(label: &str, ok: bool) {
    if ok {
        println!("  {} {}", "✓".green(), label);
    } else {
        println!("  {} {}", "✗".red(), label);
    }
}
