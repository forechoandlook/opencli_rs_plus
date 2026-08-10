//! Dispatch logic for built-in (non-adapter) commands.

use clap::ArgMatches;
use opencli_rs_core::CliError;

use crate::commands::{doctor, uninstall, update};
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
        "uninstall" => {
            if let Err(err) = uninstall::run_uninstall() {
                eprintln!("Uninstall failed: {err}");
                std::process::exit(1);
            }
            true
        }
        _ => false,
    }
}
