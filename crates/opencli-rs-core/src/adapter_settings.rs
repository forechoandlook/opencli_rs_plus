//! Persist which adapters are disabled.
//!
//! File: `~/.opencli-rs/adapter_settings.json`
//!
//! Names are `"site command"` (space-separated). A bare `"site"` entry disables
//! the whole family. Slash form `"site/command"` is accepted and normalized.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Settings file stored at `~/.opencli-rs/adapter_settings.json`.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct AdapterSettings {
    /// Disabled adapters: `"site command"` or whole-site `"site"`.
    #[serde(default)]
    pub disabled: Vec<String>,

    /// Legacy field (ignored). Older builds wrote `hidden`; keep for parse compat.
    #[serde(default, skip_serializing)]
    pub hidden: Vec<String>,
}

impl AdapterSettings {
    pub fn path() -> PathBuf {
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

    pub fn save(&self) -> Result<(), String> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        // Only persist disabled; drop legacy hidden on write.
        let out = AdapterSettings {
            disabled: self.disabled.clone(),
            hidden: Vec::new(),
        };
        let s = serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?;
        fs::write(&path, s).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Normalize user input: trim, collapse spaces, `/` → space.
    pub fn normalize_name(name: &str) -> String {
        name.trim()
            .replace('/', " ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Whether this full command name (`"site cmd"`) is disabled.
    pub fn is_disabled(&self, full_name: &str) -> bool {
        let full = Self::normalize_name(full_name);
        if self.disabled.iter().any(|d| Self::normalize_name(d) == full) {
            return true;
        }
        let site = full.split_once(' ').map(|(s, _)| s).unwrap_or(full.as_str());
        self.disabled.iter().any(|d| {
            let d = Self::normalize_name(d);
            !d.contains(' ') && d == site
        })
    }

    /// Disable one adapter or a whole site. Returns true if newly disabled.
    pub fn disable(&mut self, name: &str) -> Result<bool, String> {
        let name = Self::normalize_name(name);
        if name.is_empty() {
            return Err("adapter name is empty".into());
        }
        if self.disabled.iter().any(|d| Self::normalize_name(d) == name) {
            return Ok(false);
        }
        self.disabled.push(name);
        self.disabled.sort();
        self.disabled.dedup();
        self.save()?;
        Ok(true)
    }

    /// Enable one adapter or a whole site. Returns true if something was removed.
    pub fn enable(&mut self, name: &str) -> Result<bool, String> {
        let name = Self::normalize_name(name);
        if name.is_empty() {
            return Err("adapter name is empty".into());
        }
        let before = self.disabled.len();
        self.disabled
            .retain(|d| Self::normalize_name(d) != name);
        if self.disabled.len() != before {
            self.save()?;
            return Ok(true);
        }
        Ok(false)
    }
}
