//! Main adapter-execution entry point.
//!
//! Responsibilities:
//!   1. Handle the `--daemon` flag (browser-daemon mode, spawned by BrowserBridge).
//!   2. Load the adapter registry.
//!   3. Build the CLI, parse args.
//!   4. Route to built-in commands (via `dispatch::dispatch_builtin`) or adapter execution.

use opencli_rs_core::{AdapterSettings, CliError, Registry};
use opencli_rs_discovery::{discover_adapters, scan_dir_no_cache};
use opencli_rs_output::format::{OutputFormat, RenderOptions};
use opencli_rs_output::render;
use std::collections::HashMap;
use std::str::FromStr;

use crate::args::coerce_and_validate_args;
use crate::cli_builder::build_cli;
use crate::dispatch::{dispatch_builtin, print_error};
use opencli_rs_engine::execute_command;

/// Main adapter-execution entry point. Assumes tracing is already initialized.
pub async fn run() {
    // ── Fast-path meta flags that should not trigger adapter discovery ─────
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!(
            "opencli {} ({})",
            env!("CARGO_PKG_VERSION"),
            env!("OPENCLI_GIT_COMMIT")
        );
        return;
    }

    // ── Browser-daemon mode (spawned internally by BrowserBridge) ──────────
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

    // ── Load adapter registry ──────────────────────────────────────────────
    let mut registry = Registry::new();
    match discover_adapters(&mut registry) {
        Ok(n) => tracing::debug!(count = n, "Discovered adapters"),
        Err(e) => tracing::warn!(error = %e, "Failed to discover adapters"),
    }

    // Installed plugins replace stale legacy definitions in
    // ~/.opencli-rs/adapters. Adapter development happens in plugin repositories.
    match load_plugin_adapters(&mut registry) {
        Ok(n) if n > 0 => tracing::debug!(count = n, "Loaded plugin adapters"),
        Ok(_) => {}
        Err(e) => tracing::warn!(error = %e, "Failed to load plugin adapters"),
    }

    // Drop disabled adapters so they disappear from help and cannot run in direct mode.
    let settings = AdapterSettings::load();
    if !settings.disabled.is_empty() {
        registry.retain(|cmd| !settings.is_disabled(&cmd.full_name()));
        tracing::debug!(
            disabled = settings.disabled.len(),
            remaining = registry.command_count(),
            "Applied adapter disable list"
        );
    }

    // ── Parse args ─────────────────────────────────────────────────────────
    let app = build_cli(&registry);
    let matches = app.get_matches();

    let format_str = matches.get_one::<String>("format").unwrap().clone();
    let fields = matches.get_one::<String>("fields").map(|s| {
        s.split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>()
    });
    let verbose = matches.get_flag("verbose");
    if verbose {
        tracing::info!("Verbose mode enabled");
    }
    let output_format = OutputFormat::from_str(&format_str).unwrap_or_default();

    // ── Route to subcommand ────────────────────────────────────────────────
    let Some((site_name, site_matches)) = matches.subcommand() else {
        eprintln!("opencli v{}", env!("CARGO_PKG_VERSION"));
        eprintln!("No command specified. Use --help for usage.");
        std::process::exit(1);
    };

    // Try built-ins first
    if dispatch_builtin(site_name, site_matches, &registry).await {
        return;
    }

    // Adapter execution: `opencli <site> <command> [args...]`
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
                        fields: fields.clone(),
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
                    if verbose {
                        let secs = start.elapsed().as_secs_f64();
                        eprintln!(
                            "Elapsed: {:.2}s | Source: {}",
                            secs,
                            cmd.full_name()
                        );
                    }
                    println!("{}", render(&data, &opts));
                }
                Err(e) => {
                    let e = e.classify();
                    if e.is_soft_empty() {
                        eprintln!("{} {}", e.icon(), e);
                        let empty = serde_json::json!([]);
                        let opts = RenderOptions {
                            format: output_format,
                            fields: fields.clone(),
                            columns: None,
                            title: None,
                            elapsed: None,
                            source: None,
                            footer_extra: None,
                        };
                        println!("{}", render(&empty, &opts));
                        std::process::exit(0);
                    }
                    if format_str == "json" {
                        eprintln!("{}", e.to_json());
                    } else {
                        print_error(&e);
                    }
                    std::process::exit(e.exit_code());
                }
            }
        } else {
            eprintln!("Unknown command: {} {}", site_name, cmd_name);
            std::process::exit(1);
        }
    } else {
        // `opencli <site>` with no subcommand → show site-level help
        let app = build_cli(&registry);
        let _ = app.try_get_matches_from(vec!["opencli", site_name, "--help"]);
    }
}

/// Direct mode cannot depend on `opencli-rs-daemon` (the daemon already
/// depends on this crate), so mirror its read-only plugin loading here.
fn load_plugin_adapters(registry: &mut Registry) -> Result<usize, CliError> {
    let plugins_dir = dirs::home_dir()
        .map(|home| home.join(".opencli-rs").join("plugins"))
        .unwrap_or_else(|| std::path::PathBuf::from(".opencli-rs/plugins"));
    if !plugins_dir.exists() {
        return Ok(0);
    }

    let mut total = 0;
    for entry in std::fs::read_dir(plugins_dir)?.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || !path.is_dir() {
            continue;
        }
        match scan_dir_no_cache(&path, registry) {
            Ok(count) => {
                total += count;
                tracing::debug!(plugin = %name, adapters = count, "Loaded plugin adapters");
            }
            Err(error) => {
                tracing::warn!(plugin = %name, error = %error, "Failed to load plugin adapters");
            }
        }
    }
    Ok(total)
}
