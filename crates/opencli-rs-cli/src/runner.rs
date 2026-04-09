//! Core adapter-execution entry point. Called from both the standalone binary
//! and the unified `opencli` binary in the daemon crate.

use clap::{Arg, ArgAction, Command};
use clap_complete::Shell;
use opencli_rs_core::Registry;
use opencli_rs_discovery::{discover_adapters, scan_dir_no_cache};
use opencli_rs_output::format::{OutputFormat, RenderOptions};
use opencli_rs_output::render;
use serde_json::Value;
use std::collections::HashMap;
use std::str::FromStr;

use crate::args::coerce_and_validate_args;
use crate::commands::{completion, doctor, feedback, update};
use crate::execution::execute_command;

fn build_cli(registry: &Registry) -> Command {
    let mut app = Command::new("opencli")
        .version(env!("CARGO_PKG_VERSION"))
        .about("AI-driven CLI tool — turns websites into command-line interfaces")
        .arg(
            Arg::new("format")
                .long("format")
                .short('f')
                .global(true)
                .default_value("table")
                .help("Output format: table, json, yaml, csv, md"),
        )
        .arg(
            Arg::new("verbose")
                .long("verbose")
                .short('v')
                .global(true)
                .action(ArgAction::SetTrue)
                .help("Enable verbose output"),
        );

    for site in registry.list_sites() {
        let mut site_cmd = Command::new(site.to_string());
        for cmd in registry.list_commands(site) {
            let mut sub = Command::new(cmd.name.clone()).about(cmd.description.clone());
            for arg_def in &cmd.args {
                let mut arg = if arg_def.positional {
                    Arg::new(arg_def.name.clone())
                } else {
                    Arg::new(arg_def.name.clone()).long(arg_def.name.clone())
                };
                if let Some(desc) = &arg_def.description {
                    arg = arg.help(desc.clone());
                }
                if arg_def.required {
                    arg = arg.required(true);
                }
                if let Some(default) = &arg_def.default {
                    let default_str = match default {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    arg = arg.default_value(default_str);
                }
                sub = sub.arg(arg);
            }
            site_cmd = site_cmd.subcommand(sub);
        }
        app = app.subcommand(site_cmd);
    }

    app = app
        .subcommand(Command::new("doctor").about("Run diagnostics checks"))
        .subcommand(
            Command::new("update")
                .about("Check the latest version and update this binary in place")
                .arg(
                    Arg::new("check")
                        .long("check")
                        .action(ArgAction::SetTrue)
                        .help("Only check for updates without installing"),
                ),
        )
        .subcommand(
            Command::new("feedback")
                .about("Save local feedback and optionally open a GitHub issue draft")
                .arg(
                    Arg::new("title")
                        .required(true)
                        .help("Short feedback title"),
                )
                .arg(
                    Arg::new("body")
                        .long("body")
                        .short('m')
                        .help("Detailed feedback text"),
                )
                .arg(
                    Arg::new("adapter")
                        .long("adapter")
                        .help("Related adapter, e.g. 'zhihu hot'"),
                )
                .arg(
                    Arg::new("kind")
                        .long("kind")
                        .default_value("other")
                        .value_parser(["broken", "bad_description", "other"])
                        .help("Feedback kind"),
                )
                .arg(
                    Arg::new("open")
                        .long("open")
                        .action(ArgAction::SetTrue)
                        .help("Open a prefilled GitHub issue in the browser"),
                ),
        )
        .subcommand(
            Command::new("completion")
                .about("Generate shell completions")
                .arg(
                    Arg::new("shell")
                        .required(true)
                        .value_parser(clap::value_parser!(Shell))
                        .help("Target shell: bash, zsh, fish, powershell"),
                ),
        )
        .subcommand(
            Command::new("explore")
                .about("Explore a website's API surface and discover endpoints")
                .arg(Arg::new("url").required(true).help("URL to explore"))
                .arg(Arg::new("site").long("site").help("Override site name"))
                .arg(
                    Arg::new("goal")
                        .long("goal")
                        .help("Hint for capability naming"),
                )
                .arg(
                    Arg::new("wait")
                        .long("wait")
                        .default_value("3")
                        .help("Initial wait seconds"),
                )
                .arg(
                    Arg::new("auto")
                        .long("auto")
                        .action(ArgAction::SetTrue)
                        .help("Enable interactive fuzzing"),
                )
                .arg(
                    Arg::new("click")
                        .long("click")
                        .help("Comma-separated labels to click"),
                ),
        )
        .subcommand(
            Command::new("cascade")
                .about("Auto-detect authentication strategy for an API endpoint")
                .arg(
                    Arg::new("url")
                        .required(true)
                        .help("API endpoint URL to probe"),
                ),
        )
        .subcommand(
            Command::new("generate")
                .about("One-shot: explore + synthesize + select best adapter")
                .arg(
                    Arg::new("url")
                        .required(true)
                        .help("URL to generate adapter for"),
                )
                .arg(Arg::new("goal").long("goal").help("What you want"))
                .arg(Arg::new("site").long("site").help("Override site name")),
        )
        .subcommand(
            Command::new("summary")
                .about("Show adapter summaries")
                .subcommand(
                    Command::new("show")
                        .about("Show details of a specific adapter")
                        .arg(Arg::new("adapter").required(true).help("Adapter name")),
                ),
        );

    app
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

fn read_summary_content(summaries_dir: &std::path::Path, adapter: &str) -> Option<String> {
    let path = summaries_dir.join(format!("{}.md", adapter));
    std::fs::read_to_string(&path).ok()
}

fn parse_description_from_summary(content: &str) -> String {
    content
        .lines()
        .find(|l| l.trim().starts_with("description:"))
        .and_then(|l| l.splitn(2, ':').nth(1))
        .map(|s| s.trim().trim_matches('"').trim())
        .unwrap_or("")
        .to_string()
}

fn run_summary() {
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
        if let Some(content) = read_summary_content(&dir, adapter) {
            println!("{}", content);
            return;
        }
    }
    let local = std::path::PathBuf::from("adapters")
        .join(adapter)
        .join("summary.md");
    if local.exists() {
        if let Ok(content) = std::fs::read_to_string(&local) {
            println!("{}", content);
            return;
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        let user = std::path::PathBuf::from(home)
            .join(".opencli-rs")
            .join("adapters")
            .join(adapter)
            .join("summary.md");
        if user.exists() {
            if let Ok(content) = std::fs::read_to_string(&user) {
                println!("{}", content);
                return;
            }
        }
    }
    eprintln!("Adapter '{}' not found in summaries.", adapter);
    std::process::exit(1);
}

fn print_error(err: &opencli_rs_core::CliError) {
    eprintln!("{} {}", err.icon(), err);
    let suggestions = err.suggestions();
    if !suggestions.is_empty() {
        eprintln!();
        for s in suggestions {
            eprintln!("  -> {}", s);
        }
    }
}

/// Main adapter-execution entry point. Assumes tracing is already initialized.
pub async fn run() {
    // Check for --daemon flag (browser-daemon spawning by BrowserBridge)
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--daemon") {
        let port: u16 = {
            let mut port = None;
            let mut iter = args.iter();
            while let Some(arg) = iter.next() {
                if arg == "--port" {
                    if let Some(port_str) = iter.next() {
                        port = port_str.parse().ok();
                        break;
                    }
                }
            }
            port.or_else(|| {
                std::env::var("OPENCLI_DAEMON_PORT")
                    .ok()
                    .and_then(|s| s.parse().ok())
            })
            .unwrap_or(19825)
        };
        tracing::info!(port, "Starting browser daemon");
        match opencli_rs_browser::Daemon::start(port).await {
            Ok(daemon) => {
                tokio::signal::ctrl_c().await.ok();
                tracing::info!("Shutting down browser daemon");
                let _ = daemon.shutdown().await;
            }
            Err(e) => {
                eprintln!("Failed to start browser daemon: {}", e);
                std::process::exit(1);
            }
        }
        return;
    }

    let mut registry = Registry::new();
    match discover_adapters(&mut registry) {
        Ok(n) => tracing::debug!(count = n, "Discovered adapters"),
        Err(e) => tracing::warn!(error = %e, "Failed to discover adapters"),
    }

    let local_adapters_dir = std::path::PathBuf::from("adapters");
    if local_adapters_dir.exists() && local_adapters_dir.is_dir() {
        match scan_dir_no_cache(&local_adapters_dir, &mut registry) {
            Ok(n) if n > 0 => eprintln!("[dev] Loaded {} adapters from ./adapters/", n),
            Ok(_) => {}
            Err(e) => tracing::warn!(error = %e, "Failed to load local dev adapters"),
        }
    }

    let app = build_cli(&registry);
    let matches = app.get_matches();

    let format_str = matches.get_one::<String>("format").unwrap().clone();
    let verbose = matches.get_flag("verbose");
    if verbose {
        tracing::info!("Verbose mode enabled");
    }
    let output_format = OutputFormat::from_str(&format_str).unwrap_or_default();

    if let Some((site_name, site_matches)) = matches.subcommand() {
        match site_name {
            "doctor" => {
                doctor::run_doctor().await;
                return;
            }
            "update" => {
                let check_only = site_matches.get_flag("check");
                if let Err(err) = update::run_update(check_only).await {
                    eprintln!("Update failed: {err}");
                    std::process::exit(1);
                }
                return;
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
                return;
            }
            "completion" => {
                let shell = site_matches
                    .get_one::<Shell>("shell")
                    .copied()
                    .expect("shell argument required");
                let mut app = build_cli(&registry);
                completion::run_completion(&mut app, shell);
                return;
            }
            "explore" => {
                let url = site_matches.get_one::<String>("url").unwrap();
                let site = site_matches.get_one::<String>("site").cloned();
                let goal = site_matches.get_one::<String>("goal").cloned();
                let wait: u64 = site_matches
                    .get_one::<String>("wait")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(3);
                let auto_fuzz = site_matches.get_flag("auto");
                let click_labels: Vec<String> = site_matches
                    .get_one::<String>("click")
                    .map(|s| s.split(',').map(|l| l.trim().to_string()).collect())
                    .unwrap_or_default();
                let mut bridge = opencli_rs_browser::BrowserBridge::new(
                    std::env::var("OPENCLI_DAEMON_PORT")
                        .ok()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(19825),
                );
                match bridge.connect().await {
                    Ok(page) => {
                        let options = opencli_rs_ai::ExploreOptions {
                            timeout: Some(120),
                            max_scrolls: Some(3),
                            capture_network: Some(true),
                            wait_seconds: Some(wait as f64),
                            auto_fuzz: Some(auto_fuzz),
                            click_labels,
                            goal,
                            site_name: site,
                        };
                        let result = opencli_rs_ai::explore(page.as_ref(), url, options).await;
                        let _ = page.close().await;
                        match result {
                            Ok(manifest) => println!(
                                "{}",
                                serde_json::to_string_pretty(&manifest).unwrap_or_default()
                            ),
                            Err(e) => {
                                print_error(&e);
                                std::process::exit(1);
                            }
                        }
                    }
                    Err(e) => {
                        print_error(&e);
                        std::process::exit(1);
                    }
                }
                return;
            }
            "cascade" => {
                let url = site_matches.get_one::<String>("url").unwrap();
                let mut bridge = opencli_rs_browser::BrowserBridge::new(
                    std::env::var("OPENCLI_DAEMON_PORT")
                        .ok()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(19825),
                );
                match bridge.connect().await {
                    Ok(page) => {
                        let result = opencli_rs_ai::cascade(page.as_ref(), url).await;
                        let _ = page.close().await;
                        match result {
                            Ok(r) => {
                                println!("{}", serde_json::to_string_pretty(&r).unwrap_or_default())
                            }
                            Err(e) => {
                                print_error(&e);
                                std::process::exit(1);
                            }
                        }
                    }
                    Err(e) => {
                        print_error(&e);
                        std::process::exit(1);
                    }
                }
                return;
            }
            "summary" => {
                if let Some(("show", sub_matches)) = site_matches.subcommand() {
                    let adapter = sub_matches.get_one::<String>("adapter").unwrap();
                    run_summary_show(adapter);
                    return;
                }
                run_summary();
                return;
            }
            "generate" => {
                let url = site_matches.get_one::<String>("url").unwrap();
                let goal = site_matches.get_one::<String>("goal").cloned();
                let mut bridge = opencli_rs_browser::BrowserBridge::new(
                    std::env::var("OPENCLI_DAEMON_PORT")
                        .ok()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(19825),
                );
                match bridge.connect().await {
                    Ok(page) => {
                        let gen_result = opencli_rs_ai::generate(
                            page.as_ref(),
                            url,
                            goal.as_deref().unwrap_or(""),
                        )
                        .await;
                        let _ = page.close().await;
                        match gen_result {
                            Ok(candidate) => {
                                let home = std::env::var("HOME")
                                    .or_else(|_| std::env::var("USERPROFILE"))
                                    .unwrap_or_else(|_| ".".to_string());
                                let dir = std::path::PathBuf::from(&home)
                                    .join(".opencli-rs")
                                    .join("adapters")
                                    .join(&candidate.site);
                                let _ = std::fs::create_dir_all(&dir);
                                let path = dir.join(format!("{}.yaml", candidate.name));
                                match std::fs::write(&path, &candidate.yaml) {
                                    Ok(_) => {
                                        eprintln!(
                                            "✅ Generated adapter: {} {}",
                                            candidate.site, candidate.name
                                        );
                                        eprintln!(
                                            "   Strategy: {:?}, Confidence: {:.0}%",
                                            candidate.strategy,
                                            candidate.confidence * 100.0
                                        );
                                        eprintln!("   Saved to: {}", path.display());
                                        eprintln!("\n   Run it now:");
                                        eprintln!(
                                            "   opencli {} {}",
                                            candidate.site, candidate.name
                                        );
                                    }
                                    Err(e) => {
                                        eprintln!("Generated but failed to save: {}", e);
                                        println!("{}", candidate.yaml);
                                    }
                                }
                            }
                            Err(e) => {
                                print_error(&e);
                                std::process::exit(1);
                            }
                        }
                    }
                    Err(e) => {
                        print_error(&e);
                        std::process::exit(1);
                    }
                }
                return;
            }
            _ => {}
        }

        if let Some((cmd_name, cmd_matches)) = site_matches.subcommand() {
            if let Some(cmd) = registry.get(site_name, cmd_name) {
                let mut raw_args: HashMap<String, String> = HashMap::new();
                for arg_def in &cmd.args {
                    if let Some(val) = cmd_matches.get_one::<String>(&arg_def.name) {
                        raw_args.insert(arg_def.name.clone(), val.clone());
                    }
                }
                let kwargs = match coerce_and_validate_args(&cmd.args, &raw_args) {
                    Ok(kw) => kw,
                    Err(e) => {
                        print_error(&e);
                        std::process::exit(1);
                    }
                };
                let start = std::time::Instant::now();
                match execute_command(cmd, kwargs).await {
                    Ok(data) => {
                        let opts = RenderOptions {
                            format: output_format,
                            columns: if cmd.columns.is_empty() {
                                None
                            } else {
                                Some(cmd.columns.clone())
                            },
                            title: None,
                            elapsed: Some(start.elapsed()),
                            source: Some(cmd.full_name()),
                            footer_extra: None,
                        };
                        println!("{}", render(&data, &opts));
                    }
                    Err(e) => {
                        print_error(&e);
                        std::process::exit(1);
                    }
                }
            } else {
                eprintln!("Unknown command: {} {}", site_name, cmd_name);
                std::process::exit(1);
            }
        } else {
            let app = build_cli(&registry);
            let _ = app.try_get_matches_from(vec!["opencli", site_name, "--help"]);
        }
    } else {
        eprintln!("opencli v{}", env!("CARGO_PKG_VERSION"));
        eprintln!("No command specified. Use --help for usage.");
        std::process::exit(1);
    }
}
