//! CLI structure: build the clap Command tree and related helpers.
//!
//! Subcommands are ordered into three visual groups via `display_order`:
//!   0-9   → TOOLS    (local, no daemon required)
//!   10-19 → AI       (need browser connection)
//!   20-29 → DAEMON   (require `opencli daemon` to be running)
//!
//! Adapter site subcommands are hidden; discovered via `opencli <site> --help`.

use clap::{Arg, ArgAction, Command};
use opencli_rs_core::Registry;
use serde_json::Value;

// display_order buckets
const ORD_TOOLS: usize = 0;
const ORD_DAEMON: usize = 20;

// ──────────────────────────────────────────────────────────────────────────────
// Daemon-client command definitions (shared between runner.rs and build_cli)
// ──────────────────────────────────────────────────────────────────────────────

pub fn daemon_help_commands() -> Vec<Command> {
    vec![
        Command::new("daemon")
            .about("[daemon] Start the scheduler daemon (foreground)")
            .long_about(
                "Start the scheduler daemon. Blocks in the foreground; use nohup or a \
                 process manager to run in the background.\n\n\
                 Example:\n  nohup opencli daemon > ~/.opencli-rs/daemon.log 2>&1 &",
            )
            .display_order(ORD_DAEMON)
            .arg(
                Arg::new("poll_interval")
                    .long("poll-interval")
                    .default_value("10")
                    .help("Job polling interval in seconds"),
            )
            .arg(
                Arg::new("db")
                    .long("db")
                    .help("Override path to the scheduler SQLite database"),
            )
            .arg(
                Arg::new("addr")
                    .long("addr")
                    .help("TCP listen address, e.g. 127.0.0.1:10008"),
            ),
        Command::new("status")
            .about("[daemon] Show daemon health: adapters loaded, jobs pending")
            .display_order(ORD_DAEMON + 1),
        Command::new("stop")
            .about("[daemon] Stop the running daemon gracefully")
            .display_order(ORD_DAEMON + 2),
        Command::new("restart")
            .about("[daemon] Stop and restart the daemon")
            .display_order(ORD_DAEMON + 3)
            .arg(
                Arg::new("poll_interval")
                    .long("poll-interval")
                    .default_value("10")
                    .help("Job polling interval in seconds"),
            )
            .arg(
                Arg::new("db")
                    .long("db")
                    .help("Override path to the scheduler SQLite database"),
            ),
        Command::new("job")
            .about("[daemon] Manage scheduled jobs (add / list / show / cancel / delete)")
            .display_order(ORD_DAEMON + 4)
            .subcommand(
                Command::new("add")
                    .about("Schedule an adapter to run once or repeatedly")
                    .long_about(
                        "Schedule an adapter to run once or on a repeating interval.\n\n\
                         Job life-cycle:  pending → running → done | failed | cancelled\n\n\
                         Examples:\n  \
                           opencli job add zhihu/hot\n  \
                           opencli job add zhihu/hot --delay 300           # run in 5 min\n  \
                           opencli job add zhihu/hot --interval 3600       # run hourly\n  \
                           opencli job add twitter/search --args '{\"query\":\"rust\"}'",
                    )
                    .arg(
                        Arg::new("adapter")
                            .required(true)
                            .help("Adapter to schedule: 'site/command', e.g. zhihu/hot"),
                    )
                    .arg(Arg::new("run_at").short('r').long("run-at").help(
                        "Absolute run time (ISO 8601 or 'now'). \
                                 Mutually exclusive with --delay. \
                                 Examples: 2026-01-01T09:00:00, now",
                    ))
                    .arg(Arg::new("delay").short('d').long("delay").help(
                        "Run after N seconds from now. \
                                 Mutually exclusive with --run-at.",
                    ))
                    .arg(Arg::new("interval").short('i').long("interval").help(
                        "Repeat every N seconds. \
                                 Omit for a one-shot job. \
                                 Example: --interval 3600 for hourly.",
                    ))
                    .arg(Arg::new("args").short('a').long("args").help(
                        "Adapter arguments as a JSON object. \
                                 Example: --args '{\"query\":\"rust\",\"limit\":20}'",
                    )),
            )
            .subcommand(
                Command::new("list")
                    .about("List jobs, optionally filtered by status")
                    .arg(
                        Arg::new("status").short('s').long("status").help(
                            "Filter by status: pending | running | done | failed | cancelled",
                        ),
                    )
                    .arg(
                        Arg::new("limit")
                            .short('l')
                            .long("limit")
                            .default_value("50")
                            .help("Maximum number of jobs to return"),
                    ),
            )
            .subcommand(
                Command::new("show")
                    .about("Show full details of a job")
                    .arg(
                        Arg::new("id")
                            .required(true)
                            .help("Job ID (a unique prefix is sufficient)"),
                    ),
            )
            .subcommand(
                Command::new("cancel")
                    .about("Cancel a pending job (record is kept for auditing)")
                    .long_about(
                        "Mark a pending job as cancelled. The job record stays in the \
                         database. Use 'delete' to remove it entirely.",
                    )
                    .arg(
                        Arg::new("id")
                            .required(true)
                            .help("Job ID (a unique prefix is sufficient)"),
                    ),
            )
            .subcommand(
                Command::new("delete")
                    .about("Delete a job record permanently from the database")
                    .long_about(
                        "Permanently remove a job. \
                         Use 'cancel' if you want to stop execution but keep the history.",
                    )
                    .arg(
                        Arg::new("id")
                            .required(true)
                            .help("Job ID (a unique prefix is sufficient)"),
                    ),
            )
            .subcommand(
                Command::new("run")
                    .about("Force-trigger all due jobs immediately (useful for testing)"),
            ),
        Command::new("adapter")
            .about("[daemon] Manage adapters (list / search / enable / disable / sync)")
            .display_order(ORD_DAEMON + 5)
            .subcommand(
                Command::new("list")
                    .about("List all adapters known to the daemon")
                    .arg(
                        Arg::new("include_disabled")
                            .long("include-disabled")
                            .action(ArgAction::SetTrue)
                            .help("Also show disabled adapters"),
                    )
                    .arg(
                        Arg::new("include_hidden")
                            .long("include-hidden")
                            .action(ArgAction::SetTrue)
                            .help("Also show hidden adapters"),
                    ),
            )
            .subcommand(
                Command::new("search")
                    .about("Search adapters (BM25 + usage rank; falls back to local scan)")
                    .long_about(
                        "Full-text search over adapter name, description, domain, and summary. \
                         Score = 0.7 × BM25 + 0.3 × log(1 + usage_count).\n\n\
                         Falls back to a local file-system scan when the daemon is not running.",
                    )
                    .arg(Arg::new("query").required(true).help("Search query")),
            )
            .subcommand(
                Command::new("enable")
                    .about("Re-enable a disabled adapter")
                    .arg(
                        Arg::new("name")
                            .required(true)
                            .help("Adapter full name, e.g. 'zhihu hot'"),
                    ),
            )
            .subcommand(
                Command::new("disable")
                    .about("Disable an adapter (excludes from search and execution)")
                    .arg(
                        Arg::new("name")
                            .required(true)
                            .help("Adapter full name, e.g. 'zhihu hot'"),
                    ),
            )
            .subcommand(
                Command::new("sync")
                    .about("Sync adapters from a folder and rebuild the search index")
                    .arg(
                        Arg::new("folder")
                            .short('f')
                            .long("folder")
                            .help("Folder path (defaults to ~/.opencli-rs/adapters)"),
                    ),
            ),
        Command::new("plugin")
            .about("[daemon] Manage adapter plugins (install / uninstall / list / update)")
            .display_order(ORD_DAEMON + 6)
            .subcommand(
                Command::new("install")
                    .about("Install a plugin from GitHub or a local path")
                    .long_about(
                        "Install a plugin containing additional adapters.\n\n\
                         Source formats:\n  \
                           user/repo              GitHub shorthand\n  \
                           user/repo/subpath      GitHub subdirectory\n  \
                           github:user/repo       Explicit GitHub prefix\n  \
                           https://...            Any git URL\n  \
                           /local/path            Local directory\n  \
                           file:///path           Local directory (URI form)",
                    )
                    .arg(
                        Arg::new("path")
                            .required(true)
                            .help("Source: user/repo, https://..., or /local/path"),
                    ),
            )
            .subcommand(
                Command::new("uninstall")
                    .about("Uninstall a plugin by name")
                    .arg(Arg::new("name").required(true).help("Plugin name")),
            )
            .subcommand(Command::new("list").about("List all installed plugins"))
            .subcommand(
                Command::new("update")
                    .about("Update a plugin, or all plugins if name is omitted")
                    .arg(
                        Arg::new("name").help("Plugin name; omit to update all installed plugins"),
                    ),
            ),
        Command::new("socket")
            .about("[daemon] Send a raw JSON-RPC command to the daemon socket (debug)")
            .display_order(ORD_DAEMON + 7)
            .arg(Arg::new("args").num_args(1..).trailing_var_arg(true)),
    ]
}

// ──────────────────────────────────────────────────────────────────────────────
// Adapter catalog helper
// ──────────────────────────────────────────────────────────────────────────────

pub fn render_adapter_catalog(registry: &Registry) -> String {
    let mut lines = Vec::new();
    lines.push(String::from("Adapter families:"));
    for site in registry.list_sites() {
        let count = registry.list_commands(site).len();
        lines.push(format!("  {site:<15} {count} command(s)"));
    }
    lines.push(String::new());
    lines.push(String::from(
        "Use `opencli <site> --help` to inspect commands for one adapter family.",
    ));
    lines.join("\n")
}

// ──────────────────────────────────────────────────────────────────────────────
// Main CLI builder
// ──────────────────────────────────────────────────────────────────────────────

pub fn build_cli(registry: &Registry) -> Command {
    let mut app = Command::new("opencli")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Turn any website into a CLI — YAML adapters + shared browser session")
        .long_about(
            "USAGE:\n  \
               opencli <site> <command> [args...]     run an adapter\n  \
               opencli <site> --help                  list commands for a site\n  \
               opencli <site> <command> --help        show command arguments\n\n\
             EXAMPLES:\n  \
               opencli zhihu hot\n  \
               opencli twitter search --query openai\n  \
               opencli adapters\n  \
               opencli adapter search zhihu\n  \
               opencli job add zhihu/hot --interval 3600",
        )
        .arg(
            Arg::new("format")
                .long("format")
                .short('f')
                .global(true)
                .default_value("csv")
                .help("Output format: csv | table | json | yaml | md"),
        )
        .arg(
            Arg::new("fields")
                .long("fields")
                .global(true)
                .value_delimiter(',')
                .help("Return only these top-level fields (comma-separated)"),
        )
        .arg(
            Arg::new("verbose")
                .long("verbose")
                .short('v')
                .global(true)
                .action(ArgAction::SetTrue)
                .help("Enable verbose/debug output"),
        );

    // ── Adapter site subcommands (hidden; discovered via `opencli <site> --help`)
    for site in registry.list_sites() {
        let command_count = registry.list_commands(site).len();
        let mut site_cmd = Command::new(site.to_string())
            .about(format!("{command_count} adapter command(s) for {site}"))
            .hide(true)
            .after_help(
                "Use `opencli <site> <command> --help` to inspect adapter-specific arguments.",
            );
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

    // ── Daemon management commands (display_order 20+)
    for daemon_cmd in daemon_help_commands() {
        app = app.subcommand(daemon_cmd);
    }

    // ── Local tools — no daemon, no browser (display_order 0+)
    app = app
        .subcommand(
            Command::new("adapters")
                .about("List all installed adapter families and command counts")
                .display_order(ORD_TOOLS),
        )
        .subcommand(
            Command::new("tools")
                .about("Browse the local CLI tool knowledge base")
                .display_order(ORD_TOOLS + 1)
                .subcommand(
                    Command::new("search")
                        .about("Search tools by keyword")
                        .arg(Arg::new("query").required(true)),
                )
                .subcommand(Command::new("list").about("List all tools"))
                .subcommand(
                    Command::new("info")
                        .about("Show full details for a tool")
                        .arg(Arg::new("name").required(true)),
                )
                .subcommand(Command::new("summary").about("Show one-line summaries for all tools")),
        )
        .subcommand(
            Command::new("doctor")
                .about("Check runtime dependencies and environment")
                .display_order(ORD_TOOLS + 2),
        )
        .subcommand(
            Command::new("update")
                .about("Check for a newer release and update this binary in place")
                .display_order(ORD_TOOLS + 3)
                .arg(
                    Arg::new("check")
                        .long("check")
                        .action(ArgAction::SetTrue)
                        .help("Only check; do not install"),
                ),
        )
        .subcommand(
            Command::new("uninstall")
                .about("Remove the current opencli binary from disk")
                .display_order(ORD_TOOLS + 4)
                .long_about(
                    "Attempt to remove the currently running opencli binary from disk.\n\n\
                     This is best-effort: it works on Unix-like systems where the running \
                     executable can be unlinked, but not on Windows while the binary is in use.\n\n\
                     If opencli was installed through a package manager or symlink, remove \
                     that wrapper separately.",
                ),
        )
        .subcommand(
            Command::new("feedback")
                .about("Record feedback and optionally open a GitHub issue draft")
                .display_order(ORD_TOOLS + 5)
                .arg(
                    Arg::new("title")
                        .required(true)
                        .help("Short description, e.g. 'zhihu hot returns 403'"),
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
                        .help("Feedback category"),
                )
                .arg(
                    Arg::new("open")
                        .long("open")
                        .action(ArgAction::SetTrue)
                        .help("Open a prefilled GitHub issue in the browser"),
                ),
        )
        .subcommand(
            Command::new("summary")
                .about("Browse adapter summaries")
                .display_order(ORD_TOOLS + 6)
                .subcommand(
                    Command::new("show")
                        .about("Show the summary for a specific adapter")
                        .arg(
                            Arg::new("adapter")
                                .required(true)
                                .help("Adapter name, e.g. 'zhihu'"),
                        ),
                ),
        );

    app
}
