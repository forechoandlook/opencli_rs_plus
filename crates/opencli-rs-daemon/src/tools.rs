//! Tools knowledge base: loaded from ./tools/*.md and ~/.opencli-rs/tools/*.md
//!
//! Each file is a Markdown document with YAML frontmatter:
//!
//! ```markdown
//! ---
//! name: ripgrep
//! binary: rg
//! homepage: https://github.com/BurntSushi/ripgrep
//! tags: [search, grep, regex]
//! install:
//!   mac: brew install ripgrep
//!   linux: apt install ripgrep
//! ---
//!
//! Fast line-oriented regex search. (first paragraph = short description)
//! ```
//!
//! No database. All operations are in-memory on the loaded file list.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

// ── Types ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolFrontmatter {
    pub name: String,
    pub binary: String,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// install commands keyed by platform: mac / linux / windows / default
    #[serde(default)]
    pub install: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub binary: String,
    pub description: String,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub install: HashMap<String, String>,
    /// Full markdown body (excluding frontmatter)
    pub body: String,
}

impl Tool {
    pub fn install_cmd(&self) -> Option<&str> {
        let platform = if cfg!(target_os = "macos") {
            "mac"
        } else if cfg!(target_os = "linux") {
            "linux"
        } else {
            "windows"
        };
        self.install
            .get(platform)
            .or_else(|| self.install.get("default"))
            .map(|s| s.as_str())
    }

    pub fn is_installed(&self) -> bool {
        let cmd = if cfg!(target_os = "windows") {
            "where"
        } else {
            "which"
        };
        std::process::Command::new(cmd)
            .arg(&self.binary)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

// ── Loader ─────────────────────────────────────────────────────────────────

fn home_tools_dir() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".opencli-rs").join("tools"))
        .unwrap_or_else(|| PathBuf::from(".opencli-rs/tools"))
}

fn current_tools_dir() -> Option<PathBuf> {
    std::env::current_dir().ok().map(|cwd| cwd.join("tools"))
}

fn load_tools_from_dir(dir: &PathBuf) -> Vec<Tool> {
    if !dir.exists() || !dir.is_dir() {
        return vec![];
    }

    let mut tools = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return vec![],
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Some(tool) = parse_tool_md(&content) {
                tools.push(tool);
            }
        }
    }

    tools
}

/// Load all tools from ./tools/*.md and ~/.opencli-rs/tools/*.md
pub fn load_tools() -> Vec<Tool> {
    let mut tools = Vec::new();

    if let Some(dir) = current_tools_dir() {
        tools.extend(load_tools_from_dir(&dir));
    }

    tools.extend(load_tools_from_dir(&home_tools_dir()));

    let mut seen = std::collections::HashSet::new();
    tools.retain(|tool| seen.insert(tool.name.clone()));

    tools.sort_by(|a, b| a.name.cmp(&b.name));
    tools
}

/// Parse a markdown file with YAML frontmatter.
/// Short description = first non-empty paragraph in the body.
fn parse_tool_md(content: &str) -> Option<Tool> {
    let content = content.trim_start();
    if !content.starts_with("---") {
        return None;
    }

    // Find closing ---
    let after_open = content.get(3..)?;
    let close = after_open.find("\n---")?;
    let yaml = &after_open[..close];
    let body_start = close + 4; // skip \n---
    let body = after_open
        .get(body_start..)
        .unwrap_or("")
        .trim_start_matches('\n')
        .to_string();

    let fm: ToolFrontmatter = serde_yaml::from_str(yaml).ok()?;

    // Short description: first non-empty line in body
    let description = body
        .lines()
        .map(|l| l.trim())
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .to_string();

    Some(Tool {
        name: fm.name,
        binary: fm.binary,
        description,
        homepage: fm.homepage,
        tags: fm.tags,
        install: fm.install,
        body,
    })
}

// ── Query functions ────────────────────────────────────────────────────────

/// Search tools by keyword (matches name, binary, description, tags).
pub fn search<'a>(query: &str, tools: &'a [Tool]) -> Vec<&'a Tool> {
    if query.trim().is_empty() {
        return tools.iter().collect();
    }
    let q = query.to_lowercase();
    tools
        .iter()
        .filter(|t| {
            t.name.to_lowercase().contains(&q)
                || t.binary.to_lowercase().contains(&q)
                || t.description.to_lowercase().contains(&q)
                || t.tags.iter().any(|tag| tag.to_lowercase().contains(&q))
        })
        .collect()
}

/// Find a tool by exact name.
pub fn find_by_name<'a>(name: &str, tools: &'a [Tool]) -> Option<&'a Tool> {
    tools.iter().find(|t| t.name == name)
}

/// Summary: just name + short description for every tool.
#[derive(Debug, Serialize)]
pub struct ToolSummary {
    pub name: String,
    pub description: String,
    pub installed: bool,
}

pub fn summary(tools: &[Tool]) -> Vec<ToolSummary> {
    tools
        .iter()
        .map(|t| ToolSummary {
            name: t.name.clone(),
            description: t.description.clone(),
            installed: t.is_installed(),
        })
        .collect()
}
