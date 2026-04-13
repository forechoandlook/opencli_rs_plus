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

fn find_summaries_dir() -> Option<std::path::PathBuf> {
    let local = std::path::PathBuf::from("summaries");
    if local.exists() && local.is_dir() {
        return Some(local);
    }
    if let Ok(home) = std::env::var("HOME") {
        let user = std::path::PathBuf::from(home)
            .join(".opencli-rs")
            .join("summaries");
        if user.exists() && user.is_dir() {
            return Some(user);
        }
    }
    None
}

fn parse_description_from_summary(content: &str) -> String {
    content
        .lines()
        .find(|l| l.trim().starts_with("description:"))
        .and_then(|l| l.split_once(':').map(|x| x.1))
        .map(|s| s.trim().trim_matches('"').trim())
        .unwrap_or("")
        .to_string()
}

fn run_summary_list() {
    let mut adapters_sorted: Vec<(String, String)> = Vec::new();

    if let Some(dir) = find_summaries_dir() {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("md") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        let adapter_name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                        let description = parse_description_from_summary(&content);
                        if !description.is_empty() {
                            adapters_sorted.push((adapter_name.to_string(), description));
                        }
                    }
                }
            }
        }
    }

    let adapter_dirs = std::path::PathBuf::from("adapters");
    if adapter_dirs.exists() {
        if let Ok(entries) = std::fs::read_dir(&adapter_dirs) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let summary_path = path.join("summary.md");
                    if summary_path.exists() {
                        if let Ok(content) = std::fs::read_to_string(&summary_path) {
                            let adapter_name =
                                path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                            let description = parse_description_from_summary(&content);
                            if !description.is_empty() {
                                adapters_sorted.push((adapter_name.to_string(), description));
                            }
                        }
                    }
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
    if let Some(dir) = find_summaries_dir() {
        let path = dir.join(format!("{}.md", adapter));
        if let Ok(content) = std::fs::read_to_string(&path) {
            println!("{}", content);
            return;
        }
    }

    let local = std::path::PathBuf::from("adapters")
        .join(adapter)
        .join("summary.md");
    if let Ok(content) = std::fs::read_to_string(&local) {
        println!("{}", content);
        return;
    }

    if let Ok(home) = std::env::var("HOME") {
        let user = std::path::PathBuf::from(home)
            .join(".opencli-rs")
            .join("adapters")
            .join(adapter)
            .join("summary.md");
        if let Ok(content) = std::fs::read_to_string(&user) {
            println!("{}", content);
            return;
        }
    }

    eprintln!("Adapter '{}' not found in summaries.", adapter);
    std::process::exit(1);
}
