use opencli_rs_core::{CliError, Registry};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

use crate::yaml_parser::parse_yaml_adapter;

/// Returns the adapters directory path (~/.opencli-rs/adapters/).
pub fn adapters_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".opencli-rs").join("adapters")
}

/// Returns the cache file path (~/.opencli-rs/adapters_cache.json).
fn cache_path() -> PathBuf {
    adapters_dir().join("cache.json")
}

/// Cache entry for a single adapter file.
#[derive(Debug, Serialize, Deserialize)]
struct CacheEntry {
    /// Relative path from adapters_dir, e.g. "bilibili/hot.yaml"
    path: String,
    /// Raw YAML content
    yaml: String,
}

/// The on-disk cache file.
#[derive(Debug, Serialize, Deserialize)]
struct Cache {
    /// mtime of the adapters directory at scan time
    dir_mtime: u64,
    entries: Vec<CacheEntry>,
}

/// Returns the mtime of the adapters directory, or 0 if it doesn't exist.
fn dir_mtime() -> u64 {
    fs::metadata(adapters_dir())
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Returns true if the cache exists and the adapters directory hasn't changed since.
fn cache_valid() -> bool {
    let cache_file = cache_path();
    if !cache_file.exists() {
        return false;
    }
    let Ok(cache_content) = fs::read_to_string(&cache_file) else {
        return false;
    };
    let Ok(cache) = serde_json::from_str::<Cache>(&cache_content) else {
        return false;
    };
    cache.dir_mtime == dir_mtime()
}

/// Load commands from the cache file (fast path).
fn load_from_cache(registry: &mut Registry) -> Result<usize, CliError> {
    let cache_content = fs::read_to_string(cache_path())?;
    let cache: Cache = serde_json::from_str(&cache_content)?;

    let mut count = 0;
    for entry in cache.entries {
        match parse_yaml_adapter(&entry.yaml) {
            Ok(mut cmd) => {
                cmd.func = None; // func can't be serialized, always None for YAML adapters
                tracing::debug!(site = %cmd.site, name = %cmd.name, "Loaded from cache");
                registry.register(cmd);
                count += 1;
            }
            Err(e) => {
                tracing::warn!(path = %entry.path, error = %e, "Failed to parse cached adapter");
            }
        }
    }
    tracing::info!(count = count, "Loaded adapters from cache");
    Ok(count)
}

/// Recursively scan the adapters directory, parse YAML files, and write the cache.
fn scan_and_cache(registry: &mut Registry) -> Result<usize, CliError> {
    let dir = adapters_dir();
    let mut entries = Vec::new();
    let mut count = 0;

    scan_dir_recursive(&dir, &dir, &mut entries, registry, &mut count)?;

    // Write cache
    let cache = Cache {
        dir_mtime: dir_mtime(),
        entries,
    };
    let cache_content = serde_json::to_string(&cache).map_err(CliError::Json)?;
    fs::write(cache_path(), cache_content)?;

    tracing::info!(count = count, dest = %cache_path().display(), "Scanned and cached adapters");
    Ok(count)
}

/// Recursively scan a directory for YAML files.
fn scan_dir_recursive(
    base: &PathBuf,
    current: &PathBuf,
    entries: &mut Vec<CacheEntry>,
    registry: &mut Registry,
    count: &mut usize,
) -> Result<(), CliError> {
    if !current.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            scan_dir_recursive(base, &path, entries, registry, count)?;
        } else if path
            .extension()
            .is_some_and(|e| e == "yaml" || e == "yml")
        {
            let rel = path
                .strip_prefix(base)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");

            let yaml = fs::read_to_string(&path)?;
            entries.push(CacheEntry {
                path: rel,
                yaml: yaml.clone(),
            });

            match parse_yaml_adapter(&yaml) {
                Ok(mut cmd) => {
                    cmd.func = None;
                    tracing::debug!(site = %cmd.site, name = %cmd.name, "Registered adapter");
                    registry.register(cmd);
                    *count += 1;
                }
                Err(e) => {
                    tracing::warn!(path = ?path, error = %e, "Failed to parse adapter");
                }
            }
        }
    }
    Ok(())
}

/// Scan a directory directly without any caching — used for local dev adapters.
pub fn scan_dir_no_cache(dir: &PathBuf, registry: &mut Registry) -> Result<usize, CliError> {
    let mut count = 0;
    if dir.exists() {
        scan_dir_recursive(dir, dir, &mut Vec::new(), registry, &mut count)?;
    }
    Ok(count)
}

/// Discover and register all adapters.
/// Uses a cache file to skip re-parsing if the adapters directory hasn't changed.
pub fn discover_adapters(registry: &mut Registry) -> Result<usize, CliError> {
    let dir = adapters_dir();

    // Empty or missing directory — nothing to load
    if !dir.exists()
        || fs::read_dir(&dir)
            .map(|mut d| d.next().is_none())
            .unwrap_or(true)
    {
        return Ok(0);
    }

    // Fast path: cache is valid, load from it
    if cache_valid() {
        return load_from_cache(registry);
    }

    // Slow path: scan directory and rebuild cache
    scan_and_cache(registry)
}
