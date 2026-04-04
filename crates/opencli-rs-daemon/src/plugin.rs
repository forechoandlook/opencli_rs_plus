//! Plugin manager: install, uninstall, update, and list plugins.
//!
//! Plugins live in ~/.opencli-rs/plugins/<name>/.
//! Lock file: ~/.opencli-rs/plugins.lock.json
//!
//! Install sources:
//!   github:user/repo           → clone https://github.com/user/repo.git
//!   github:user/repo/subpath   → clone repo, install only subpath/ as the plugin
//!   https://...                → clone URL directly
//!   git@host:user/repo.git     → clone SSH URL
//!   file:///absolute/path      → symlink (Unix) or copy (Windows)
//!   local:/path                → same as file://
//!   /absolute/path             → same as file://

use anyhow::Result;
use chrono::Utc;
use opencli_rs_core::Registry;
use opencli_rs_discovery::scan_dir_no_cache;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};
use uuid::Uuid;

// ── Types ─────────────────────────────────────────────────────────────────────

/// Parsed from `opencli-plugin.json` at the plugin root.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginManifest {
    #[serde(default)]
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    /// Semver range for opencli-rs compatibility (informational only).
    pub opencli: Option<String>,
}

/// One entry in the lock file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginLockEntry {
    pub source: String,
    pub installed_at: String,
}

/// Returned by `list()`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub source: String,
    pub installed_at: String,
    pub dir: String,
}

type PluginLock = HashMap<String, PluginLockEntry>;

// ── Source kinds ──────────────────────────────────────────────────────────────

#[derive(Debug)]
enum SourceKind {
    /// clone_url, optional subpath inside the repo
    Git {
        clone_url: String,
        subpath: Option<String>,
    },
    Local(PathBuf),
}

// ── PluginManager ─────────────────────────────────────────────────────────────

pub struct PluginManager {
    /// ~/.opencli-rs/plugins/
    pub plugins_dir: PathBuf,
    /// ~/.opencli-rs/plugins.lock.json
    lock_path: PathBuf,
}

impl PluginManager {
    pub fn new() -> Self {
        let base = dirs::home_dir()
            .map(|h| h.join(".opencli-rs"))
            .unwrap_or_else(|| PathBuf::from(".opencli-rs"));
        Self {
            plugins_dir: base.join("plugins"),
            lock_path: base.join("plugins.lock.json"),
        }
    }

    // ── Lock file ─────────────────────────────────────────────────────────────

    fn load_lock(&self) -> PluginLock {
        if !self.lock_path.exists() {
            return HashMap::new();
        }
        fs::read_to_string(&self.lock_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn save_lock(&self, lock: &PluginLock) -> Result<()> {
        if let Some(parent) = self.lock_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let s = serde_json::to_string_pretty(lock)?;
        fs::write(&self.lock_path, s)?;
        Ok(())
    }

    fn upsert_lock(&self, name: &str, source: &str) -> Result<()> {
        let mut lock = self.load_lock();
        lock.insert(
            name.to_string(),
            PluginLockEntry {
                source: source.to_string(),
                installed_at: Utc::now().to_rfc3339(),
            },
        );
        self.save_lock(&lock)
    }

    // ── Manifest ──────────────────────────────────────────────────────────────

    fn read_manifest(&self, dir: &Path) -> PluginManifest {
        let path = dir.join("opencli-plugin.json");
        if path.exists() {
            if let Ok(s) = fs::read_to_string(&path) {
                if let Ok(mut m) = serde_json::from_str::<PluginManifest>(&s) {
                    if m.name.is_empty() {
                        m.name = dir
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("unknown")
                            .to_string();
                    }
                    return m;
                }
            }
        }
        // Synthesize from dir name
        PluginManifest {
            name: dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string(),
            ..Default::default()
        }
    }

    // ── Source parsing ────────────────────────────────────────────────────────

    fn parse_source(source: &str) -> SourceKind {
        // github:user/repo  or  github:user/repo/subpath
        if let Some(repo) = source.strip_prefix("github:") {
            let parts: Vec<&str> = repo.splitn(3, '/').collect();
            let clone_url = format!(
                "https://github.com/{}/{}.git",
                parts.first().copied().unwrap_or(""),
                parts.get(1).copied().unwrap_or(""),
            );
            let subpath = parts
                .get(2)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());
            return SourceKind::Git { clone_url, subpath };
        }
        // Full HTTPS or SSH URL (no subpath support for raw URLs)
        if source.starts_with("https://")
            || source.starts_with("git@")
            || source.starts_with("ssh://")
        {
            return SourceKind::Git {
                clone_url: source.to_string(),
                subpath: None,
            };
        }
        // Local path: file:///path, local:/path, or bare /absolute/path
        let path_str = source
            .strip_prefix("file://")
            .or_else(|| source.strip_prefix("local:"))
            .unwrap_or(source);
        SourceKind::Local(PathBuf::from(path_str))
    }

    // ── Build PluginInfo ──────────────────────────────────────────────────────

    fn plugin_info(&self, dir: &Path, source: &str, installed_at: &str) -> PluginInfo {
        let manifest = self.read_manifest(dir);
        PluginInfo {
            name: manifest.name,
            version: manifest.version,
            description: manifest.description,
            source: source.to_string(),
            installed_at: installed_at.to_string(),
            dir: dir.display().to_string(),
        }
    }

    // ── Public API ────────────────────────────────────────────────────────────

    /// Install a plugin from a source specifier.
    pub async fn install(&self, source: &str) -> Result<PluginInfo> {
        fs::create_dir_all(&self.plugins_dir)?;

        match Self::parse_source(source) {
            SourceKind::Git { clone_url, subpath } => {
                self.install_git(source, &clone_url, subpath.as_deref())
                    .await
            }
            SourceKind::Local(path) => self.install_local(source, &path),
        }
    }

    async fn install_git(
        &self,
        original_source: &str,
        clone_url: &str,
        subpath: Option<&str>,
    ) -> Result<PluginInfo> {
        let tmp_name = format!(".tmp-{}", Uuid::new_v4());
        let tmp_dir = self.plugins_dir.join(&tmp_name);
        let url = clone_url.to_string();
        let tmp = tmp_dir.clone();

        // Shell out to git clone (blocking, so run in spawn_blocking)
        let clone_result = tokio::task::spawn_blocking(move || {
            let out = std::process::Command::new("git")
                .args(["clone", "--depth", "1", &url, tmp.to_str().unwrap_or("")])
                .output()
                .map_err(|e| anyhow::anyhow!("failed to run git: {}", e))?;
            if out.status.success() {
                Ok(())
            } else {
                Err(anyhow::anyhow!(
                    "git clone failed: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                ))
            }
        })
        .await?;

        if let Err(e) = clone_result {
            let _ = fs::remove_dir_all(&tmp_dir);
            return Err(e);
        }

        // Effective plugin root: repo root, or subpath inside repo
        let plugin_root = match subpath {
            Some(sp) => {
                let sub = tmp_dir.join(sp);
                if !sub.is_dir() {
                    let _ = fs::remove_dir_all(&tmp_dir);
                    return Err(anyhow::anyhow!("subpath '{}' not found in repository", sp));
                }
                sub
            }
            None => tmp_dir.clone(),
        };

        let manifest = self.read_manifest(&plugin_root);
        let dest = self.plugins_dir.join(&manifest.name);

        let result = (|| -> Result<()> {
            if dest.exists() {
                fs::remove_dir_all(&dest)?;
            }
            if subpath.is_some() {
                // Only copy the subpath dir; tmp_dir (full clone) goes away after
                copy_dir_all(&plugin_root, &dest)?;
            } else {
                // Move the whole clone (preserves .git for future git pull)
                fs::rename(&tmp_dir, &dest)?;
            }
            Ok(())
        })();

        // Always clean up the full clone temp dir (rename moved it; copy left it behind)
        let _ = fs::remove_dir_all(&tmp_dir);

        if let Err(e) = result {
            return Err(e);
        }

        let now = Utc::now().to_rfc3339();
        self.upsert_lock(&manifest.name, original_source)?;
        info!(
            plugin = %manifest.name,
            url = %clone_url,
            subpath = subpath.unwrap_or("(root)"),
            "Plugin installed from git"
        );
        Ok(self.plugin_info(&dest, original_source, &now))
    }

    fn install_local(&self, original_source: &str, src_path: &Path) -> Result<PluginInfo> {
        if !src_path.exists() {
            return Err(anyhow::anyhow!(
                "local plugin path does not exist: {}",
                src_path.display()
            ));
        }
        if !src_path.is_dir() {
            return Err(anyhow::anyhow!(
                "local plugin path is not a directory: {}",
                src_path.display()
            ));
        }

        let manifest = self.read_manifest(src_path);
        let dest = self.plugins_dir.join(&manifest.name);

        if dest.exists() {
            return Err(anyhow::anyhow!(
                "plugin '{}' is already installed at {}",
                manifest.name,
                dest.display()
            ));
        }

        #[cfg(unix)]
        {
            let abs = src_path
                .canonicalize()
                .unwrap_or_else(|_| src_path.to_path_buf());
            std::os::unix::fs::symlink(&abs, &dest)?;
        }
        #[cfg(not(unix))]
        copy_dir_all(src_path, &dest)?;

        let now = Utc::now().to_rfc3339();
        self.upsert_lock(&manifest.name, original_source)?;
        info!(plugin = %manifest.name, path = %src_path.display(), "Plugin installed from local");
        Ok(self.plugin_info(&dest, original_source, &now))
    }

    /// Uninstall a plugin by name.
    pub async fn uninstall(&self, name: &str) -> Result<()> {
        let plugin_dir = self.plugins_dir.join(name);
        if plugin_dir.exists() {
            // Use symlink_metadata to detect symlinks without following them
            let meta = fs::symlink_metadata(&plugin_dir)?;
            if meta.file_type().is_symlink() {
                fs::remove_file(&plugin_dir)?;
            } else {
                fs::remove_dir_all(&plugin_dir)?;
            }
        }
        let mut lock = self.load_lock();
        lock.remove(name);
        self.save_lock(&lock)?;
        info!(plugin = %name, "Plugin uninstalled");
        Ok(())
    }

    /// List all installed plugins.
    pub fn list(&self) -> Result<Vec<PluginInfo>> {
        if !self.plugins_dir.exists() {
            return Ok(vec![]);
        }
        let lock = self.load_lock();
        let mut plugins = vec![];

        let entries = fs::read_dir(&self.plugins_dir)?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();

            // Skip hidden dirs (temp installs) and non-dirs
            if name.starts_with('.') {
                continue;
            }
            if !path.is_dir() {
                continue;
            }

            let manifest = self.read_manifest(&path);
            let lock_entry = lock.get(&name);
            let source = lock_entry
                .map(|e| e.source.as_str())
                .unwrap_or("")
                .to_string();
            let installed_at = lock_entry
                .map(|e| e.installed_at.as_str())
                .unwrap_or("")
                .to_string();

            plugins.push(PluginInfo {
                name: manifest.name,
                version: manifest.version,
                description: manifest.description,
                source,
                installed_at,
                dir: path.display().to_string(),
            });
        }
        Ok(plugins)
    }

    /// Update a plugin.
    ///
    /// - Full-repo git install (`github:user/repo`): `git pull --ff-only` in place.
    /// - Subpath git install (`github:user/repo/sub`): re-clone and re-copy,
    ///   because the plugin dir contains only the subpath (no `.git`).
    /// - Local: no-op (symlink points to live source).
    pub async fn update(&self, name: &str) -> Result<()> {
        let plugin_dir = self.plugins_dir.join(name);
        if !plugin_dir.exists() {
            return Err(anyhow::anyhow!("plugin '{}' not found", name));
        }
        let lock = self.load_lock();
        let source = lock.get(name).map(|e| e.source.clone()).unwrap_or_default();

        match Self::parse_source(&source) {
            SourceKind::Git {
                clone_url: _,
                subpath: Some(_),
            } => {
                // Subpath install: no .git in plugin dir — re-install from scratch
                info!(plugin = %name, "Subpath plugin — re-cloning to update");
                self.install_git(
                    &source,
                    &Self::clone_url_from_source(&source),
                    Self::subpath_from_source(&source).as_deref(),
                )
                .await?;
            }
            SourceKind::Git {
                clone_url: _,
                subpath: None,
            } => {
                // Full-repo install: .git is present — git pull in place
                let dir = plugin_dir.clone();
                let out = tokio::task::spawn_blocking(move || {
                    std::process::Command::new("git")
                        .args(["-C", dir.to_str().unwrap_or(""), "pull", "--ff-only"])
                        .output()
                        .map_err(|e| anyhow::anyhow!("failed to run git: {}", e))
                })
                .await??;

                if !out.status.success() {
                    return Err(anyhow::anyhow!(
                        "git pull failed: {}",
                        String::from_utf8_lossy(&out.stderr).trim()
                    ));
                }
                info!(plugin = %name, "Plugin updated via git pull");
            }
            SourceKind::Local(_) => {
                debug!(plugin = %name, "Local plugin — nothing to update");
            }
        }
        Ok(())
    }

    /// Extract clone URL from a source string (re-parse without subpath).
    fn clone_url_from_source(source: &str) -> String {
        if let Some(repo) = source.strip_prefix("github:") {
            let parts: Vec<&str> = repo.splitn(3, '/').collect();
            return format!(
                "https://github.com/{}/{}.git",
                parts.first().copied().unwrap_or(""),
                parts.get(1).copied().unwrap_or(""),
            );
        }
        source.to_string()
    }

    /// Extract subpath from a `github:user/repo/subpath` source string.
    fn subpath_from_source(source: &str) -> Option<String> {
        if let Some(repo) = source.strip_prefix("github:") {
            let parts: Vec<&str> = repo.splitn(3, '/').collect();
            return parts
                .get(2)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());
        }
        None
    }

    /// Update all installed plugins.
    pub async fn update_all(&self) -> Vec<(String, Result<()>)> {
        let plugins = match self.list() {
            Ok(p) => p,
            Err(e) => {
                warn!(error = %e, "Failed to list plugins for update_all");
                return vec![];
            }
        };
        let mut results = vec![];
        for p in plugins {
            let result = self.update(&p.name).await;
            results.push((p.name, result));
        }
        results
    }

    /// Load all plugin adapter YAML files into the given registry.
    pub fn load_into_registry(&self, registry: &mut Registry) -> Result<usize> {
        if !self.plugins_dir.exists() {
            return Ok(0);
        }
        let mut total = 0usize;
        let entries = fs::read_dir(&self.plugins_dir)?;
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || !path.is_dir() {
                continue;
            }
            match scan_dir_no_cache(&path, registry) {
                Ok(n) => {
                    total += n;
                    debug!(plugin = %name, adapters = n, "Loaded plugin adapters");
                }
                Err(e) => {
                    warn!(plugin = %name, error = %e, "Failed to load plugin adapters");
                }
            }
        }
        Ok(total)
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Recursive directory copy.
fn copy_dir_all(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let dst_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &dst_path)?;
        } else {
            fs::copy(entry.path(), &dst_path)?;
        }
    }
    Ok(())
}
