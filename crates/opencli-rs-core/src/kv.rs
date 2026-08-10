//! Local key-value store for small, durable identity/session hints.
//!
//! Path: `~/.opencli-rs/kv.json`
//!
//! Intended for stable values such as `xiaohongshu:me.userId`, not bulk scrape
//! results or secrets (cookies/tokens). Values are JSON; TTL is optional.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const STORE_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KvEntry {
    pub value: Value,
    /// Unix seconds when written.
    pub updated_at: u64,
    /// Unix seconds when this entry expires; omit/null means no expiry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct KvFile {
    #[serde(default = "default_version")]
    version: u32,
    #[serde(default)]
    entries: BTreeMap<String, KvEntry>,
}

fn default_version() -> u32 {
    STORE_VERSION
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Default store path: `~/.opencli-rs/kv.json`.
pub fn kv_path() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".opencli-rs").join("kv.json"))
        .unwrap_or_else(|| PathBuf::from("kv.json"))
}

fn load_file(path: &Path) -> KvFile {
    let Ok(raw) = fs::read_to_string(path) else {
        return KvFile::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

fn save_file(path: &Path, store: &KvFile) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("kv: create dir: {e}"))?;
    }
    let body = serde_json::to_string_pretty(store).map_err(|e| format!("kv: encode: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    {
        let mut f = fs::File::create(&tmp).map_err(|e| format!("kv: write temp: {e}"))?;
        f.write_all(body.as_bytes())
            .map_err(|e| format!("kv: write temp: {e}"))?;
        f.sync_all().ok();
    }
    fs::rename(&tmp, path).map_err(|e| format!("kv: rename: {e}"))?;
    Ok(())
}

fn is_expired(entry: &KvEntry, now: u64) -> bool {
    entry.expires_at.is_some_and(|exp| exp <= now)
}

/// Parse TTL strings like `30d`, `24h`, `15m`, `60s`, or bare seconds (`3600`).
pub fn parse_ttl(spec: &str) -> Result<u64, String> {
    let s = spec.trim();
    if s.is_empty() {
        return Err("kv: empty ttl".into());
    }
    if let Ok(secs) = s.parse::<u64>() {
        return Ok(secs);
    }
    let (num, unit) = s.split_at(s.len().saturating_sub(1));
    let n: u64 = num
        .trim()
        .parse()
        .map_err(|_| format!("kv: invalid ttl '{spec}'"))?;
    let mult = match unit {
        "s" | "S" => 1,
        "m" | "M" => 60,
        "h" | "H" => 3600,
        "d" | "D" => 86400,
        _ => return Err(format!("kv: invalid ttl unit in '{spec}' (use s/m/h/d)")),
    };
    Ok(n.saturating_mul(mult))
}

/// Get a value. Expired entries are treated as missing and removed lazily.
pub fn get(key: &str) -> Result<Option<Value>, String> {
    get_at(&kv_path(), key)
}

pub fn get_at(path: &Path, key: &str) -> Result<Option<Value>, String> {
    let key = key.trim();
    if key.is_empty() {
        return Err("kv: empty key".into());
    }
    let mut store = load_file(path);
    let now = now_secs();
    match store.entries.get(key) {
        Some(entry) if is_expired(entry, now) => {
            store.entries.remove(key);
            let _ = save_file(path, &store);
            Ok(None)
        }
        Some(entry) => Ok(Some(entry.value.clone())),
        None => Ok(None),
    }
}

/// Set a value. Empty string values are rejected unless `allow_empty` is true.
pub fn set(key: &str, value: Value, ttl: Option<&str>) -> Result<(), String> {
    set_at(&kv_path(), key, value, ttl)
}

pub fn set_at(
    path: &Path,
    key: &str,
    value: Value,
    ttl: Option<&str>,
) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() {
        return Err("kv: empty key".into());
    }
    if value.is_null() {
        return Err("kv: refusing to store null (use del)".into());
    }
    if value.as_str() == Some("") {
        return Err("kv: refusing to store empty string".into());
    }

    let now = now_secs();
    let expires_at = match ttl {
        Some(spec) if !spec.trim().is_empty() => Some(now.saturating_add(parse_ttl(spec)?)),
        _ => None,
    };

    let mut store = load_file(path);
    store.version = STORE_VERSION;
    store.entries.insert(
        key.to_string(),
        KvEntry {
            value,
            updated_at: now,
            expires_at,
        },
    );
    save_file(path, &store)
}

/// Delete one key. Returns whether it existed.
pub fn del(key: &str) -> Result<bool, String> {
    del_at(&kv_path(), key)
}

pub fn del_at(path: &Path, key: &str) -> Result<bool, String> {
    let key = key.trim();
    if key.is_empty() {
        return Err("kv: empty key".into());
    }
    let mut store = load_file(path);
    let removed = store.entries.remove(key).is_some();
    if removed {
        save_file(path, &store)?;
    }
    Ok(removed)
}

/// List entries. Optional prefix filter. Purges expired entries.
pub fn list(prefix: Option<&str>) -> Result<Vec<(String, KvEntry)>, String> {
    list_at(&kv_path(), prefix)
}

pub fn list_at(path: &Path, prefix: Option<&str>) -> Result<Vec<(String, KvEntry)>, String> {
    let mut store = load_file(path);
    let now = now_secs();
    let before = store.entries.len();
    store.entries.retain(|_, e| !is_expired(e, now));
    if store.entries.len() != before {
        let _ = save_file(path, &store);
    }

    let mut out: Vec<(String, KvEntry)> = store
        .entries
        .into_iter()
        .filter(|(k, _)| prefix.map(|p| k.starts_with(p)).unwrap_or(true))
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// Clear all keys, or only those with a prefix.
pub fn clear(prefix: Option<&str>) -> Result<usize, String> {
    clear_at(&kv_path(), prefix)
}

pub fn clear_at(path: &Path, prefix: Option<&str>) -> Result<usize, String> {
    let mut store = load_file(path);
    let before = store.entries.len();
    match prefix {
        Some(p) if !p.is_empty() => {
            store.entries.retain(|k, _| !k.starts_with(p));
        }
        _ => store.entries.clear(),
    }
    let removed = before.saturating_sub(store.entries.len());
    if removed > 0 {
        save_file(path, &store)?;
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn tmp_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("opencli-kv-test-{name}-{nanos}.json"))
    }

    #[test]
    fn set_get_del_roundtrip() {
        let path = tmp_path("roundtrip");
        let _ = fs::remove_file(&path);
        set_at(&path, "xiaohongshu:me.userId", json!("abc123def456"), None).unwrap();
        assert_eq!(
            get_at(&path, "xiaohongshu:me.userId").unwrap(),
            Some(json!("abc123def456"))
        );
        assert!(del_at(&path, "xiaohongshu:me.userId").unwrap());
        assert_eq!(get_at(&path, "xiaohongshu:me.userId").unwrap(), None);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn ttl_expires() {
        let path = tmp_path("ttl");
        let _ = fs::remove_file(&path);
        set_at(&path, "k", json!("v"), Some("1")).unwrap();
        // Force expiry by rewriting entry
        let mut store = load_file(&path);
        if let Some(e) = store.entries.get_mut("k") {
            e.expires_at = Some(1);
        }
        save_file(&path, &store).unwrap();
        assert_eq!(get_at(&path, "k").unwrap(), None);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn list_prefix_and_clear() {
        let path = tmp_path("prefix");
        let _ = fs::remove_file(&path);
        set_at(&path, "xiaohongshu:me.userId", json!("u1"), None).unwrap();
        set_at(&path, "xiaohongshu:me.redId", json!("r1"), None).unwrap();
        set_at(&path, "bilibili:me.mid", json!("m1"), None).unwrap();
        let xs = list_at(&path, Some("xiaohongshu:")).unwrap();
        assert_eq!(xs.len(), 2);
        assert_eq!(clear_at(&path, Some("xiaohongshu:")).unwrap(), 2);
        assert_eq!(list_at(&path, None).unwrap().len(), 1);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn parse_ttl_units() {
        assert_eq!(parse_ttl("30").unwrap(), 30);
        assert_eq!(parse_ttl("2m").unwrap(), 120);
        assert_eq!(parse_ttl("1h").unwrap(), 3600);
        assert_eq!(parse_ttl("1d").unwrap(), 86400);
        assert!(parse_ttl("x").is_err());
    }
}
