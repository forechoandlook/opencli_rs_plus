//! Dispatch logic for built-in (non-adapter) commands.

use clap::ArgMatches;
use opencli_rs_core::CliError;

use crate::commands::{doctor, feedback, uninstall, update};
use opencli_rs_core::Registry;

pub fn print_error(err: &CliError) {
    eprintln!("{} {}", err.icon(), err);
    let suggestions = err.suggestions();
    if !suggestions.is_empty() {
        eprintln!();
        for s in suggestions {
            eprintln!("  -> {}", s);
        }
    }
}

/// Try to dispatch a built-in command. Returns `true` if handled, `false` if
/// this is an adapter site name that the caller should route to the registry.
pub async fn dispatch_builtin(
    site_name: &str,
    site_matches: &ArgMatches,
    _registry: &Registry,
) -> bool {
    match site_name {
        "doctor" => {
            doctor::run_doctor().await;
            true
        }
        "update" => {
            let check_only = site_matches.get_flag("check");
            if let Err(err) = update::run_update(check_only).await {
                eprintln!("Update failed: {err}");
                std::process::exit(1);
            }
            true
        }
        "feedback" => {
            let title = site_matches.get_one::<String>("title").unwrap();
            let body = site_matches.get_one::<String>("body").map(String::as_str);
            let adapter = site_matches
                .get_one::<String>("adapter")
                .map(String::as_str);
            let kind = site_matches
                .get_one::<String>("kind")
                .map(String::as_str)
                .unwrap_or("other");
            let open_issue = site_matches.get_flag("open");
            if let Err(err) = feedback::save_feedback(adapter, kind, title, body, open_issue) {
                eprintln!("Feedback failed: {err}");
                std::process::exit(1);
            }
            true
        }
        "uninstall" => {
            if let Err(err) = uninstall::run_uninstall() {
                eprintln!("Uninstall failed: {err}");
                std::process::exit(1);
            }
            true
        }
        "summary" => {
            handle_summary(site_matches);
            true
        }
        _ => false,
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Summary handler
// ──────────────────────────────────────────────────────────────────────────────

fn handle_summary(m: &ArgMatches) {
    if let Some(("show", sub)) = m.subcommand() {
        let adapter = sub.get_one::<String>("adapter").unwrap();
        run_summary_show(adapter);
        return;
    }
    run_summary_list();
}

/// Candidate adapter root dirs: local `adapters/` and `~/.opencli-rs/adapters`.
fn adapter_roots() -> Vec<std::path::PathBuf> {
    let mut roots = vec![std::path::PathBuf::from("adapters")];
    if let Ok(home) = std::env::var("HOME") {
        roots.push(
            std::path::PathBuf::from(home)
                .join(".opencli-rs")
                .join("adapters"),
        );
    }
    roots.into_iter().filter(|p| p.is_dir()).collect()
}

/// Parse a top-level `key:` value from YAML text (ignores indented/nested keys).
fn parse_top_field(content: &str, key: &str) -> Option<String> {
    let prefix = format!("{}:", key);
    content
        .lines()
        .find(|l| l.starts_with(&prefix))
        .and_then(|l| l.split_once(':').map(|x| x.1))
        .map(|s| s.trim().trim_matches('"').trim_matches('\'').trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Build the summary markdown for one adapter dir, generated from `meta.yaml`
/// (adapter-level name/description/version) plus each tool yaml's name+description.
fn build_summary(dir: &std::path::Path) -> Option<String> {
    let site = dir.file_name()?.to_str()?.to_string();

    let meta = std::fs::read_to_string(dir.join("meta.yaml")).unwrap_or_default();
    let name = parse_top_field(&meta, "name").unwrap_or_else(|| site.clone());
    let description = parse_top_field(&meta, "description").unwrap_or_default();
    let version = parse_top_field(&meta, "version").unwrap_or_else(|| "1.0.0".to_string());

    // Collect tools: every *.yaml except meta.yaml.
    let mut tools: Vec<(String, String)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("yaml") {
                continue;
            }
            if path.file_name().and_then(|s| s.to_str()) == Some("meta.yaml") {
                continue;
            }
            let content = std::fs::read_to_string(&path).unwrap_or_default();
            let tool_name = parse_top_field(&content, "name").unwrap_or_else(|| {
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string()
            });
            let tool_desc = parse_top_field(&content, "description").unwrap_or_default();
            tools.push((tool_name, tool_desc));
        }
    }
    tools.sort_by(|a, b| a.0.cmp(&b.0));

    let mut out = format!(
        "---\nname: {}\ndescription: {}\nversion: {}\n---\n工具列表:\n",
        name, description, version
    );
    for (tn, td) in tools {
        out.push_str(&format!("- {}: {}\n", tn, td));
    }
    Some(out)
}

fn run_summary_list() {
    let mut adapters_sorted: Vec<(String, String)> = Vec::new();

    for root in adapter_roots() {
        if let Ok(entries) = std::fs::read_dir(&root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let name = match path.file_name().and_then(|s| s.to_str()) {
                    Some(n) => n.to_string(),
                    None => continue,
                };
                let meta = std::fs::read_to_string(path.join("meta.yaml")).unwrap_or_default();
                let description = parse_top_field(&meta, "description").unwrap_or_default();
                if !description.is_empty() {
                    adapters_sorted.push((name, description));
                }
            }
        }
    }

    adapters_sorted.sort_by(|a, b| a.0.cmp(&b.0));
    adapters_sorted.dedup_by(|a, b| a.0 == b.0);
    for (name, desc) in adapters_sorted {
        println!("{}: {}", name, desc);
    }
}

fn run_summary_show(adapter: &str) {
    for root in adapter_roots() {
        let dir = root.join(adapter);
        if dir.is_dir() {
            if let Some(summary) = build_summary(&dir) {
                print!("{}", summary);
                return;
            }
        }
    }

    eprintln!("Adapter '{}' not found.", adapter);
    std::process::exit(1);
}
