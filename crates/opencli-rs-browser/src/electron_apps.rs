//! Electron app registry and CDP auto-discovery.
//!
//! Maps site names to their CDP debug ports. Probes whether the app is already
//! running with CDP enabled, and returns the WebSocket debugger URL.
//!
//! User-defined apps can be added via ~/.opencli-rs/apps.yaml:
//!   antigravity:
//!     port: 9234
//!     process_name: Antigravity
//!     display_name: Antigravity

use opencli_rs_core::CliError;
use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ElectronApp {
    pub port: u16,
    pub process_name: &'static str,
    pub display_name: &'static str,
}

/// Builtin Electron app registry (mirrors TS electron-apps.ts).
pub fn builtin_apps() -> HashMap<&'static str, ElectronApp> {
    let mut m = HashMap::new();
    m.insert(
        "cursor",
        ElectronApp {
            port: 9226,
            process_name: "Cursor",
            display_name: "Cursor",
        },
    );
    m.insert(
        "codex",
        ElectronApp {
            port: 9222,
            process_name: "Codex",
            display_name: "Codex",
        },
    );
    m.insert(
        "chatwise",
        ElectronApp {
            port: 9228,
            process_name: "ChatWise",
            display_name: "ChatWise",
        },
    );
    m.insert(
        "notion",
        ElectronApp {
            port: 9230,
            process_name: "Notion",
            display_name: "Notion",
        },
    );
    m.insert(
        "discord-app",
        ElectronApp {
            port: 9232,
            process_name: "Discord",
            display_name: "Discord",
        },
    );
    m.insert(
        "doubao-app",
        ElectronApp {
            port: 9225,
            process_name: "Doubao",
            display_name: "Doubao",
        },
    );
    m.insert(
        "antigravity",
        ElectronApp {
            port: 9234,
            process_name: "Antigravity",
            display_name: "Antigravity",
        },
    );
    m.insert(
        "chatgpt",
        ElectronApp {
            port: 9236,
            process_name: "ChatGPT",
            display_name: "ChatGPT",
        },
    );
    m
}

/// User-defined app entry from apps.yaml.
#[derive(Debug, Deserialize)]
struct UserApp {
    port: u16,
    process_name: String,
    #[serde(default)]
    display_name: Option<String>,
}

/// Look up the CDP port for a site. Checks builtin registry first, then
/// ~/.opencli-rs/apps.yaml for user-defined entries.
pub fn get_app_port(site: &str) -> Option<u16> {
    if let Some(app) = builtin_apps().get(site) {
        return Some(app.port);
    }
    // Load user apps.yaml
    if let Some(home) = dirs::home_dir() {
        let path = home.join(".opencli-rs").join("apps.yaml");
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(map) = serde_yaml::from_str::<HashMap<String, UserApp>>(&content) {
                if let Some(entry) = map.get(site) {
                    return Some(entry.port);
                }
            }
        }
    }
    None
}

/// CDP target descriptor from /json endpoint.
#[derive(Debug, Deserialize)]
struct CdpTarget {
    #[serde(rename = "type")]
    target_type: Option<String>,
    #[serde(rename = "webSocketDebuggerUrl")]
    ws_url: Option<String>,
    url: Option<String>,
}

/// Probe CDP endpoint and return the best WebSocket debugger URL.
/// Prefers the main page target (type=page, not devtools/extension).
pub async fn probe_cdp(port: u16) -> Result<String, CliError> {
    let http_url = format!("http://127.0.0.1:{}/json", port);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .map_err(|e| CliError::browser_connect(format!("HTTP client error: {e}")))?;

    let resp = client.get(&http_url).send().await.map_err(|e| {
        CliError::browser_connect(format!(
            "CDP not available on port {port} — launch app with --remote-debugging-port={port}: {e}"
        ))
    })?;

    let targets: Vec<CdpTarget> = resp
        .json()
        .await
        .map_err(|e| CliError::browser_connect(format!("Failed to parse CDP targets: {e}")))?;

    // Pick best target: prefer page type, skip devtools/extension urls
    let best = targets
        .iter()
        .find(|t| {
            t.ws_url.is_some()
                && t.target_type.as_deref() == Some("page")
                && !t.url.as_deref().unwrap_or("").starts_with("devtools://")
                && !t
                    .url
                    .as_deref()
                    .unwrap_or("")
                    .starts_with("chrome-extension://")
        })
        .or_else(|| targets.iter().find(|t| t.ws_url.is_some()));

    best.and_then(|t| t.ws_url.clone()).ok_or_else(|| {
        CliError::browser_connect(format!("No inspectable targets found on port {port}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_apps() {
        let apps = builtin_apps();
        assert!(apps.contains_key("cursor"));
        assert_eq!(apps.get("cursor").unwrap().port, 9226);
        assert_eq!(apps.get("cursor").unwrap().process_name, "Cursor");

        assert!(apps.contains_key("notion"));
        assert_eq!(apps.get("notion").unwrap().port, 9230);
    }

    #[test]
    fn test_get_app_port_builtin() {
        assert_eq!(get_app_port("discord-app"), Some(9232));
        assert_eq!(get_app_port("unknown-app"), None);
    }
}
