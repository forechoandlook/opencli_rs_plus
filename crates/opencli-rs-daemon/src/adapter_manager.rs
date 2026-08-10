//! Adapter manager: loads adapters from discovery, manages enabled/disabled state,
//! supports sync from arbitrary folders, and simple substring search.

use anyhow::Result;
use opencli_rs_core::{AdapterSettings, CliCommand, Registry};
use opencli_rs_discovery::{discover_adapters, scan_dir_no_cache};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::plugin::PluginManager;

/// Loaded adapter entry with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterEntry {
    pub site: String,
    pub name: String,
    pub full_name: String,
    pub description: String,
    pub domain: Option<String>,
    pub browser: bool,
    pub args: Vec<opencli_rs_core::ArgDef>,
    pub columns: Vec<String>,
    pub pipeline: Option<Vec<serde_json::Value>>,
    pub timeout_seconds: Option<u64>,
    pub version: Option<String>,
    pub updated_at: Option<String>,
    pub context: Option<opencli_rs_core::ContextAction>,
    pub enabled: bool,
}

impl AdapterEntry {
    fn from_cmd(cmd: &CliCommand, enabled: bool) -> Self {
        Self {
            site: cmd.site.clone(),
            name: cmd.name.clone(),
            full_name: cmd.full_name(),
            description: cmd.description.clone(),
            domain: cmd.domain.clone(),
            browser: cmd.browser,
            args: cmd.args.clone(),
            columns: cmd.columns.clone(),
            pipeline: cmd.pipeline.clone(),
            timeout_seconds: cmd.timeout_seconds,
            version: cmd.version.clone(),
            updated_at: cmd.updated_at.clone(),
            context: cmd.context.clone(),
            enabled,
        }
    }
}

/// Adapter manager owns the registry and settings, exposing query and mutation APIs.
pub struct AdapterManager {
    registry: RwLock<Registry>,
    plugin_manager: Arc<PluginManager>,
}

impl AdapterManager {
    /// Create a new manager, loading adapters from the default adapters directory.
    pub async fn new() -> Result<Self> {
        let settings = AdapterSettings::load();
        let mut registry = Registry::new();

        // Load built-in adapters from ~/.opencli-rs/adapters/
        let home_count = discover_adapters(&mut registry)?;

        // Load local adapters/ directory for development
        let local_dir = PathBuf::from("adapters");
        let local_count = if local_dir.exists() && local_dir.is_dir() {
            scan_dir_no_cache(&local_dir, &mut registry)?
        } else {
            0
        };

        // Load plugin adapters from ~/.opencli-rs/plugins/*/
        let plugin_manager = Arc::new(PluginManager::new());
        let plugin_count = plugin_manager
            .load_into_registry(&mut registry)
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "Failed to load plugin adapters");
                0
            });

        tracing::info!(
            home_adapters = home_count,
            local_adapters = local_count,
            plugin_adapters = plugin_count,
            disabled = settings.disabled.len(),
            "Adapter manager initialized"
        );

        Ok(Self {
            registry: RwLock::new(registry),
            plugin_manager,
        })
    }

    /// Return all adapters (including disabled), with their current enabled/disabled status.
    pub async fn list_adapters(&self) -> Vec<AdapterEntry> {
        let registry = self.registry.read().await;
        let settings = AdapterSettings::load();

        registry
            .all_commands()
            .iter()
            .map(|cmd| {
                let full_name = cmd.full_name();
                let enabled = !settings.is_disabled(&full_name);
                AdapterEntry::from_cmd(cmd, enabled)
            })
            .collect()
    }

    /// Simple case-insensitive substring match on name / site / description / domain.
    pub async fn search(&self, query: &str) -> Vec<AdapterEntry> {
        let all = self.list_adapters().await;
        let q = query.to_lowercase();
        if q.is_empty() {
            return all.into_iter().filter(|a| a.enabled).collect();
        }
        all.into_iter()
            .filter(|a| {
                a.enabled
                    && (a.full_name.to_lowercase().contains(&q)
                        || a.description.to_lowercase().contains(&q)
                        || a.site.to_lowercase().contains(&q)
                        || a.domain
                            .as_ref()
                            .map(|d| d.to_lowercase().contains(&q))
                            .unwrap_or(false))
            })
            .collect()
    }

    /// Disable an adapter (`"site command"`) or whole site (`"site"`).
    /// Returns true when the disable list contains the entry after the call.
    pub async fn disable(&self, name: &str) -> Result<bool> {
        let mut settings = AdapterSettings::load();
        settings.disable(name).map_err(|e| anyhow::anyhow!(e))?;
        let name = AdapterSettings::normalize_name(name);
        tracing::info!(adapter = %name, "Adapter disabled");
        Ok(settings
            .disabled
            .iter()
            .any(|d| AdapterSettings::normalize_name(d) == name))
    }

    /// Enable an adapter or whole-site entry.
    /// Returns true when that exact entry is no longer on the disable list.
    pub async fn enable(&self, name: &str) -> Result<bool> {
        let mut settings = AdapterSettings::load();
        settings.enable(name).map_err(|e| anyhow::anyhow!(e))?;
        let name = AdapterSettings::normalize_name(name);
        tracing::info!(adapter = %name, "Adapter enabled");
        Ok(!settings
            .disabled
            .iter()
            .any(|d| AdapterSettings::normalize_name(d) == name))
    }

    /// Full reload from default directories (including plugins).
    pub async fn reload(&self) -> Result<usize> {
        let plugin_mgr = Arc::clone(&self.plugin_manager);
        let count = {
            let mut registry = self.registry.write().await;
            *registry = Registry::new();
            let mut c = discover_adapters(&mut registry)?;
            let local_dir = PathBuf::from("adapters");
            if local_dir.exists() && local_dir.is_dir() {
                c += scan_dir_no_cache(&local_dir, &mut registry)?;
            }
            c += plugin_mgr
                .load_into_registry(&mut registry)
                .unwrap_or_else(|e| {
                    tracing::warn!(error = %e, "Failed to reload plugin adapters");
                    0
                });
            c
        };
        tracing::info!(count = count, "Adapters reloaded");
        Ok(count)
    }

    /// Expose the plugin manager for use in socket handlers.
    pub fn plugin_manager(&self) -> Arc<PluginManager> {
        Arc::clone(&self.plugin_manager)
    }

    /// Get a command by site and name, respecting enabled/disabled state.
    /// Returns None if the adapter is disabled or not found.
    pub async fn get_command(&self, site: &str, name: &str) -> Option<CliCommand> {
        let registry = self.registry.read().await;
        let settings = AdapterSettings::load();
        let full_name = format!("{} {}", site, name);

        if settings.is_disabled(&full_name) {
            return None;
        }

        registry.get(site, name).cloned()
    }

    /// Check if a command exists (even if disabled).
    #[allow(dead_code)]
    pub async fn command_exists(&self, site: &str, name: &str) -> bool {
        let registry = self.registry.read().await;
        registry.get(site, name).is_some()
    }

    #[allow(dead_code)]
    pub fn registry(&self) -> &RwLock<Registry> {
        &self.registry
    }
}

/// Check if Chrome/Chromium is running as a process.
/// Mirrors the logic from opencli-rs-browser/src/bridge.rs since that function is private.
pub fn is_chrome_running() -> bool {
    if cfg!(target_os = "macos") {
        std::process::Command::new("pgrep")
            .args(["-x", "Google Chrome"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    } else if cfg!(target_os = "windows") {
        std::process::Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq chrome.exe", "/NH"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("chrome.exe"))
            .unwrap_or(false)
    } else {
        std::process::Command::new("pgrep")
            .args(["-x", "chrome|chromium"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}
