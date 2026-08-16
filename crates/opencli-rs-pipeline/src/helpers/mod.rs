//! Shared JavaScript snippets that adapters can inject via `evaluate.helpers`.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

thread_local! {
    static HELPER_ROOT: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

pub fn set_helper_root(root: Option<PathBuf>) {
    HELPER_ROOT.with(|slot| *slot.borrow_mut() = root);
}

pub fn current_helper_root() -> Option<PathBuf> {
    HELPER_ROOT.with(|slot| slot.borrow().clone())
}

const WBI: &str = include_str!("wbi.js");
const ZHIHU_FETCH: &str = include_str!("zhihu-fetch.js");
const PINIA_WAIT: &str = include_str!("pinia-wait.js");

/// Built-in helper name → JS source.
pub fn builtin_helper(name: &str) -> Option<&'static str> {
    match name.trim().trim_end_matches(".js") {
        "wbi" | "bilibili-wbi" | "bilibili/wbi" => Some(WBI),
        "zhihu-fetch" | "zhihu_fetch" | "zhihu/fetch" => Some(ZHIHU_FETCH),
        "pinia-wait" | "pinia_wait" | "xiaohongshu/pinia-wait" => Some(PINIA_WAIT),
        _ => None,
    }
}

/// Resolve helper source: plugin file overrides the built-in of the same name.
pub fn resolve_helper(name: &str, helper_root: Option<&Path>) -> Result<String, String> {
    let trimmed = name.trim().trim_start_matches("./");
    if let Some(root) = helper_root {
        for candidate in [
            root.join(trimmed),
            root.join("helpers").join(trimmed),
            root.join(format!("{trimmed}.js")),
            root.join("helpers").join(format!("{trimmed}.js")),
        ] {
            if candidate.is_file() {
                return std::fs::read_to_string(&candidate).map_err(|e| {
                    format!("failed to read helper {}: {e}", candidate.display())
                });
            }
        }
        // site folder: look one level up (plugin root / helpers)
        if let Some(parent) = root.parent() {
            let up = parent.join("helpers").join(trimmed);
            if up.is_file() {
                return std::fs::read_to_string(&up)
                    .map_err(|e| format!("failed to read helper {}: {e}", up.display()));
            }
            let up_js = parent.join("helpers").join(format!("{trimmed}.js"));
            if up_js.is_file() {
                return std::fs::read_to_string(&up_js)
                    .map_err(|e| format!("failed to read helper {}: {e}", up_js.display()));
            }
        }
    }
    if let Some(src) = builtin_helper(trimmed) {
        return Ok(src.to_string());
    }
    Err(format!("unknown helper '{trimmed}'"))
}

pub fn helper_search_root(source_dir: Option<&PathBuf>) -> Option<PathBuf> {
    source_dir.cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_wbi_contains_sign() {
        let src = builtin_helper("wbi").unwrap();
        assert!(src.contains("function wbiSign"));
        assert!(src.contains("async function wbiGet"));
    }
}
