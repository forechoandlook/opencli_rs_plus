//! Shared helpers for dumping pipeline data to disk.
//!
//! Previously these functions were duplicated across `fetch.rs`, `browser.rs`
//! and `transform.rs`. They are now consolidated here.

use std::fs;
use std::path::Path;
use std::time::SystemTime;

use serde_json::Value;
use tracing::info;

/// Resolve template variables in a path string.
/// Supports: `{ts}` (unix seconds), `{ts_ms}` (milliseconds), `{step}` (step index).
/// Callers without a step index can pass `0`; the `{step}` token is simply
/// absent from their templates so the replacement is a no-op.
pub fn resolve_dump_path(path_tpl: &str, step_index: usize) -> String {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    path_tpl
        .replace("{ts}", &now.as_secs().to_string())
        .replace("{ts_ms}", &now.as_millis().to_string())
        .replace("{step}", &step_index.to_string())
}

/// Whether API response dumping is enabled via `OPENCLI_API_DUMP`.
pub fn api_dump_enabled() -> bool {
    matches!(
        std::env::var("OPENCLI_API_DUMP"),
        Ok(v) if matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON")
    )
}

/// Sanitize a string so it is safe to use as a filename part.
pub fn sanitize_dump_part(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    while out.contains("__") {
        out = out.replace("__", "_");
    }
    out.trim_matches('_').chars().take(80).collect()
}

/// Dump a JSON value to a file. Creates parent directories if needed.
/// - Arrays/Objects → pretty-printed JSON
/// - Primitives (string, number, bool, null) → raw value written as-is
pub fn dump_value_to_file(value: &Value, path: &Path) {
    let content = match value {
        Value::Object(_) | Value::Array(_) => {
            serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".to_string())
        }
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
    };
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            let _ = fs::create_dir_all(parent);
        }
    }
    match fs::write(path, &content) {
        Ok(_) => info!(
            path = path.to_string_lossy().as_ref(),
            "Dumped raw data to file"
        ),
        Err(e) => tracing::warn!(path = %path.display(), err = %e, "Failed to dump raw data"),
    }
}

/// Dump an API/browser response when `OPENCLI_API_DUMP` is enabled.
pub fn dump_api_response(step: &str, url: &str, value: &Value) {
    if !api_dump_enabled() {
        return;
    }
    let base_dir =
        std::env::var("OPENCLI_API_DUMP_DIR").unwrap_or_else(|_| "./data/api-dumps".to_string());
    let step_part = sanitize_dump_part(step);
    let url_part = sanitize_dump_part(url);
    let path = format!("{base_dir}/{step_part}_{url_part}_{{ts_ms}}.json");
    let resolved_path = resolve_dump_path(&path, 0);
    dump_value_to_file(value, Path::new(&resolved_path));
}
