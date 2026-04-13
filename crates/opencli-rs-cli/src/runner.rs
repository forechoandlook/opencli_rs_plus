//! Main adapter-execution entry point.
//!
//! Responsibilities:
//!   1. Handle the `--daemon` flag (browser-daemon mode, spawned by BrowserBridge).
//!   2. Load the adapter registry.
//!   3. Build the CLI, parse args.
//!   4. Route to built-in commands (via `dispatch::dispatch_builtin`) or adapter execution.

use opencli_rs_core::Registry;
use opencli_rs_discovery::{discover_adapters, scan_dir_no_cache};
use opencli_rs_output::format::{OutputFormat, RenderOptions};
use opencli_rs_output::render;
use std::collections::HashMap;
use std::str::FromStr;

use crate::args::coerce_and_validate_args;
use crate::cli_builder::build_cli;
use crate::dispatch::{dispatch_builtin, print_error};
use crate::execution::execute_command;

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

    let local_adapters_dir = std::path::PathBuf::from("adapters");
    if local_adapters_dir.exists() && local_adapters_dir.is_dir() {
        match scan_dir_no_cache(&local_adapters_dir, &mut registry) {
            Ok(n) if n > 0 => tracing::debug!(count = n, "Loaded local dev adapters"),
            Ok(_) => {}
            Err(e) => tracing::warn!(error = %e, "Failed to load local dev adapters"),
        }
    }

    // ── Parse args ─────────────────────────────────────────────────────────
    let app = build_cli(&registry);
    let matches = app.get_matches();

    let format_str = matches.get_one::<String>("format").unwrap().clone();
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
        // `opencli <site>` with no subcommand → show site-level help
        let app = build_cli(&registry);
        let _ = app.try_get_matches_from(vec!["opencli", site_name, "--help"]);
    }
}
