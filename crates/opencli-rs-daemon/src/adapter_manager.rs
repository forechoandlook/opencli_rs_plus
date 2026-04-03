//! Adapter manager: loads adapters from discovery, manages enabled/disabled state,
//! supports sync from arbitrary folders, and exposes search.

use anyhow::Result;
use opencli_rs_core::{CliCommand, Registry};
use opencli_rs_discovery::{discover_adapters, scan_dir_no_cache};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::index::AdapterIndex;
use crate::plugin::PluginManager;

/// Settings file stored at ~/.opencli-rs/adapter_settings.json
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AdapterSettings {
    /// List of "site command" names that are disabled
    #[serde(default)]
    pub disabled: Vec<String>,

    /// List of "site command" names that are hidden (not shown in help)
    #[serde(default)]
    pub hidden: Vec<String>,
}

impl AdapterSettings {
    fn path() -> PathBuf {
        dirs::home_dir()
            .map(|h| h.join(".opencli-rs").join("adapter_settings.json"))
            .unwrap_or_else(|| PathBuf::from("adapter_settings.json"))
    }

    pub fn load() -> Self {
        let path = Self::path();
        if !path.exists() {
            return Self::default();
        }
        fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let s = serde_json::to_string_pretty(self)?;
        fs::write(&path, s)?;
        Ok(())
    }
}

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
    pub timeout_seconds: Option<u64>,
    pub version: Option<String>,
    pub updated_at: Option<String>,
    pub enabled: bool,
    pub hidden: bool,
}

impl AdapterEntry {
    fn from_cmd(cmd: &CliCommand, enabled: bool, hidden: bool) -> Self {
        Self {
            site: cmd.site.clone(),
            name: cmd.name.clone(),
            full_name: cmd.full_name(),
            description: cmd.description.clone(),
            domain: cmd.domain.clone(),
            browser: cmd.browser,
            args: cmd.args.clone(),
            columns: cmd.columns.clone(),
            timeout_seconds: cmd.timeout_seconds,
            version: cmd.version.clone(),
            updated_at: cmd.updated_at.clone(),
            enabled,
            hidden,
        }
    }
}

/// Adapter manager owns the registry and settings, exposing query and mutation APIs.
pub struct AdapterManager {
    registry: RwLock<Registry>,
    settings: RwLock<AdapterSettings>,
    pub index: Arc<AdapterIndex>,
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

        // Initialize FTS index
        let index_path = dirs::home_dir()
            .map(|h| h.join(".opencli-rs").join("index.db"))
            .unwrap_or_else(|| PathBuf::from("index.db"));
        let index = Arc::new(AdapterIndex::new(index_path)?);

        let manager = Self {
            registry: RwLock::new(registry),
            settings: RwLock::new(settings),
            index,
            plugin_manager,
        };

        // Build initial FTS index (incremental: skips unchanged adapters on restart)
        let all = manager.list_adapters().await;
        manager.index.sync(&all)?;

        Ok(manager)
    }

    /// Return all adapters (including disabled), with their current enabled/disabled status.
    pub async fn list_adapters(&self) -> Vec<AdapterEntry> {
        let registry = self.registry.read().await;
        let settings = self.settings.read().await;

        registry
            .all_commands()
            .iter()
            .map(|cmd| {
                let full_name = cmd.full_name();
                let hidden = settings.hidden.contains(&full_name);
                let enabled = !settings.disabled.contains(&full_name);
                AdapterEntry::from_cmd(cmd, enabled, hidden)
            })
            .collect()
    }

    /// Return only enabled (not disabled) adapters, optionally excluding hidden ones.
    #[allow(dead_code)]
    pub async fn list_enabled(&self, include_hidden: bool) -> Vec<AdapterEntry> {
        let all = self.list_adapters().await;
        all.into_iter()
            .filter(|a| a.enabled && (include_hidden || !a.hidden))
            .collect()
    }

    /// Search adapters using FTS5/BM25 + usage hotspot hybrid ranking.
    pub async fn search(&self, query: &str, include_hidden: bool) -> Vec<AdapterEntry> {
        let fts_results = match self.index.search(query, 50) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, "FTS search failed, falling back to substring match");
                return self.search_fallback(query, include_hidden).await;
            }
        };

        let registry = self.registry.read().await;
        let settings = self.settings.read().await;

        fts_results
            .into_iter()
            .filter_map(|r| {
                let parts: Vec<&str> = r.full_name.splitn(2, ' ').collect();
                if parts.len() != 2 {
                    return None;
                }
                let cmd = registry.get(parts[0], parts[1])?;
                let hidden = settings.hidden.contains(&r.full_name);
                let enabled = !settings.disabled.contains(&r.full_name);
                if !enabled || (!include_hidden && hidden) {
                    return None;
                }
                Some(AdapterEntry::from_cmd(cmd, enabled, hidden))
            })
            .collect()
    }

    /// Fallback substring search when FTS is unavailable.
    async fn search_fallback(&self, query: &str, include_hidden: bool) -> Vec<AdapterEntry> {
        let all = self.list_adapters().await;
        let query_lower = query.to_lowercase();
        all.into_iter()
            .filter(|a| {
                a.enabled
                    && (include_hidden || !a.hidden)
                    && (a.full_name.to_lowercase().contains(&query_lower)
                        || a.description.to_lowercase().contains(&query_lower)
                        || a.site.to_lowercase().contains(&query_lower))
            })
            .collect()
    }

    /// Disable an adapter by full name ("site command").
    pub async fn disable(&self, full_name: &str) -> Result<bool> {
        let mut settings = self.settings.write().await;
        if !settings.disabled.contains(&full_name.to_string()) {
            settings.disabled.push(full_name.to_string());
            settings.save()?;
            tracing::info!(adapter = full_name, "Adapter disabled");
        }
        Ok(settings.disabled.contains(&full_name.to_string()))
    }

    /// Enable an adapter by full name ("site command").
    pub async fn enable(&self, full_name: &str) -> Result<bool> {
        let mut settings = self.settings.write().await;
        settings.disabled.retain(|d| d != full_name);
        settings.save()?;
        tracing::info!(adapter = full_name, "Adapter enabled");
        Ok(!settings.disabled.contains(&full_name.to_string()))
    }

    /// Hide an adapter (still functional but not shown in help).
    #[allow(dead_code)]
    pub async fn hide(&self, full_name: &str) -> Result<()> {
        let mut settings = self.settings.write().await;
        if !settings.hidden.contains(&full_name.to_string()) {
            settings.hidden.push(full_name.to_string());
            settings.save()?;
        }
        Ok(())
    }

    /// Unhide an adapter.
    #[allow(dead_code)]
    pub async fn unhide(&self, full_name: &str) -> Result<()> {
        let mut settings = self.settings.write().await;
        settings.hidden.retain(|h| h != full_name);
        settings.save()?;
        Ok(())
    }

    /// Sync adapters from a specific folder (replaces auto-discovery for that folder).
    /// Returns the number of adapters loaded.
    pub async fn sync_from(&self, folder: &Path) -> Result<usize> {
        let count = {
            let mut registry = self.registry.write().await;
            scan_dir_no_cache(&folder.to_path_buf(), &mut registry)?
        };
        tracing::info!(folder = %folder.display(), count = count, "Adapters synced from folder");
        let all = self.list_adapters().await;
        self.index.sync(&all)?;
        Ok(count)
    }

    /// Full reload from default directories (including plugins).
    pub async fn reload(&self) -> Result<usize> {
        let plugin_mgr = Arc::clone(&self.plugin_manager);
        let count = {
            let mut registry = self.registry.write().await;
            let mut c = discover_adapters(&mut registry)?;
            let local_dir = PathBuf::from("adapters");
            if local_dir.exists() && local_dir.is_dir() {
                c += scan_dir_no_cache(&local_dir, &mut registry)?;
            }
            c += plugin_mgr.load_into_registry(&mut registry).unwrap_or_else(|e| {
                tracing::warn!(error = %e, "Failed to reload plugin adapters");
                0
            });
            c
        };
        tracing::info!(count = count, "Adapters reloaded");
        let all = self.list_adapters().await;
        self.index.sync(&all)?;
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
        let settings = self.settings.read().await;
        let full_name = format!("{} {}", site, name);

        if settings.disabled.contains(&full_name) {
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
