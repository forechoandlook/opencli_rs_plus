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
            .about("Daemon lifecycle (start/stop/status/logs) or foreground run")
            .long_about(
                "Without a subcommand, starts the opencli daemon in the foreground \
                 (adapter/plugin management + extension API).\n\n\
                 Subcommands:\n  \
                   opencli daemon start|stop|restart|status|logs|config\n  \
                   opencli daemon autostart install|uninstall|status",
            )
            .display_order(ORD_DAEMON)
            .arg(
                Arg::new("addr")
                    .long("addr")
                    .help("TCP listen address, e.g. 127.0.0.1:10008"),
            )
            .subcommand(Command::new("start").about("Start daemon in background"))
            .subcommand(Command::new("stop").about("Stop the running daemon"))
            .subcommand(Command::new("restart").about("Restart the daemon"))
            .subcommand(Command::new("status").about("Show daemon health"))
            .subcommand(
                Command::new("logs")
                    .about("Show daemon log output")
                    .arg(
                        Arg::new("follow")
                            .short('f')
                            .long("follow")
                            .action(ArgAction::SetTrue)
                            .help("Follow log output"),
                    )
                    .arg(
                        Arg::new("lines")
                            .short('n')
                            .long("lines")
                            .default_value("50")
                            .help("Lines from end of log"),
                    ),
            )
            .subcommand(Command::new("config").about("Show daemon paths and autostart"))
            .subcommand(
                Command::new("autostart")
                    .about("Boot-time autostart (launchd / systemd --user)")
                    .subcommand(Command::new("install").about("Install and enable"))
                    .subcommand(Command::new("uninstall").about("Disable and remove"))
                    .subcommand(Command::new("status").about("Show autostart status")),
            ),
        Command::new("adapter")
            .about("Manage adapters (list / search / enable / disable)")
            .display_order(ORD_DAEMON + 1)
            .subcommand(
                Command::new("list")
                    .about("List adapters")
                    .arg(
                        Arg::new("include_disabled")
                            .long("include-disabled")
                            .action(ArgAction::SetTrue)
                            .help("Also show disabled adapters"),
                    ),
            )
            .subcommand(
                Command::new("search")
                    .about("Search adapters (substring; prints usage)")
                    .long_about(
                        "Case-insensitive substring match on site, command name, description, \
                         and domain. Prints a ready-to-copy invocation for each hit.\n\n\
                         Uses the daemon registry when available; otherwise scans local files.",
                    )
                    .arg(Arg::new("query").required(true).help("Search query")),
            )
            .subcommand(
                Command::new("enable")
                    .about("Re-enable a disabled adapter or site")
                    .arg(
                        Arg::new("name")
                            .required(true)
                            .help("Adapter: 'site command', 'site/command', or bare 'site'"),
                    ),
            )
            .subcommand(
                Command::new("disable")
                    .about("Disable an adapter or whole site (leave help + block run)")
                    .arg(
                        Arg::new("name")
                            .required(true)
                            .help("Adapter: 'site command', 'site/command', or bare 'site'"),
                    ),
            ),
        Command::new("plugin")
            .about("Manage adapter plugins (install / uninstall / list / update)")
            .display_order(ORD_DAEMON + 2)
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
        Command::new("batch")
            .about("Run an item adapter for every row from a list adapter or JSON file")
            .long_about(
                "Fetch a following/list, then run another adapter once per person.\n\n\
                 Examples:\n  \
                   opencli batch zhihu following --each user --id url_token --limit 100 --out ./archive/zhihu --all --resume\n  \
                   opencli batch xiaohongshu following --each user --id id --limit 100 --out ./archive/xhs --all\n  \
                   opencli batch bilibili following --each user-videos --id mid --limit 100 --out ./archive/bili --all\n  \
                   opencli batch zhihu following --each user --id url_token --items following.json --out ./archive/zhihu --resume",
            )
            .display_order(ORD_TOOLS + 6)
            .arg(Arg::new("site").required(true).help("Site, e.g. zhihu"))
            .arg(
                Arg::new("list_cmd")
                    .help("List adapter name, e.g. following (omit when using --items)"),
            )
            .arg(
                Arg::new("each")
                    .long("each")
                    .required(true)
                    .help("Item adapter to run per row, e.g. user"),
            )
            .arg(
                Arg::new("id")
                    .long("id")
                    .required(true)
                    .help("Field on each list row used as the item id (url_token / id / mid)"),
            )
            .arg(
                Arg::new("out")
                    .long("out")
                    .required(true)
                    .help("Output directory"),
            )
            .arg(
                Arg::new("limit")
                    .long("limit")
                    .help("Passed to the item adapter as --limit (max items per person)"),
            )
            .arg(
                Arg::new("all")
                    .long("all")
                    .action(ArgAction::SetTrue)
                    .help("Pass --all true to the list adapter"),
            )
            .arg(
                Arg::new("incremental")
                    .long("incremental")
                    .action(ArgAction::SetTrue)
                    .help("Pass --incremental true to the item adapter"),
            )
            .arg(
                Arg::new("resume")
                    .long("resume")
                    .action(ArgAction::SetTrue)
                    .help("Skip people already recorded in out/progress.json"),
            )
            .arg(
                Arg::new("sleep")
                    .long("sleep")
                    .default_value("0.4")
                    .help("Seconds to wait between people"),
            )
            .arg(
                Arg::new("items")
                    .long("items")
                    .help("JSON array file instead of running the list adapter"),
            )
            .arg(
                Arg::new("name-field")
                    .long("name-field")
                    .default_value("name")
                    .help("Display-name field on each list row"),
            ),
        Command::new("kv")
            .about("Local key-value store for identity/session hints (~/.opencli-rs/kv.json)")
            .long_about(
                "Cache small, stable identity fields across commands (e.g. bilibili:me.mid, \
                 xiaohongshu:me.userId). Do not store cookies/tokens or full scrape results.\n\n\
                 Examples:\n  \
                   opencli kv get bilibili:me.mid\n  \
                   opencli kv set xiaohongshu:me.userId 55b2... --ttl 30d\n  \
                   opencli kv list --prefix bilibili:\n  \
                   opencli kv clear --prefix xiaohongshu:\n  \
                   opencli kv clear --all",
            )
            .display_order(ORD_TOOLS + 5)
            .subcommand(
                Command::new("get")
                    .about("Read one key")
                    .arg(Arg::new("key").required(true).help("Key, e.g. bilibili:me.mid")),
            )
            .subcommand(
                Command::new("set")
                    .about("Write one key (value is a string; use --json for raw JSON)")
                    .arg(Arg::new("key").required(true).help("Key"))
                    .arg(Arg::new("value").required(true).help("Value"))
                    .arg(
                        Arg::new("ttl")
                            .long("ttl")
                            .help("Optional TTL: 30d / 24h / 15m / 60s / bare seconds"),
                    )
                    .arg(
                        Arg::new("json")
                            .long("json")
                            .action(ArgAction::SetTrue)
                            .help("Parse value as JSON instead of a plain string"),
                    ),
            )
            .subcommand(
                Command::new("list")
                    .about("List keys (optional prefix filter)")
                    .arg(
                        Arg::new("prefix")
                            .long("prefix")
                            .help("Only list keys with this prefix"),
                    ),
            )
            .subcommand(
                Command::new("del")
                    .about("Delete one key")
                    .arg(Arg::new("key").required(true).help("Key to delete")),
            )
            .subcommand(
                Command::new("clear")
                    .about("Clear keys by prefix, or entire store with --all")
                    .arg(
                        Arg::new("prefix")
                            .long("prefix")
                            .help("Only clear keys with this prefix"),
                    )
                    .arg(
                        Arg::new("all")
                            .long("all")
                            .action(ArgAction::SetTrue)
                            .help("Required when clearing the entire store"),
                    ),
            ),
    ]
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
               opencli adapter search zhihu\n  \
               opencli plugin install forechoandlook/opencli-adapters\n  \
               opencli batch zhihu following --each user --id url_token --limit 100 --out ./archive --all",
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
            let mut about = cmd.description.clone();
            let caps = &cmd.capabilities;
            let mut tags = Vec::new();
            if caps.auth {
                tags.push("auth");
            }
            if caps.paginate {
                tags.push("paginate");
            }
            if caps.incremental {
                tags.push("incremental");
            }
            if caps.download {
                tags.push("download");
            }
            if caps.rich_text {
                tags.push("rich_text");
            }
            if !tags.is_empty() {
                about = format!("{about} [{}]", tags.join(","));
            }
            let mut sub = Command::new(cmd.name.clone()).about(about);
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

    // ── Local / maintenance commands (display_order 0+)
    app = app
        .subcommand(
            Command::new("doctor")
                .about("Check runtime dependencies and environment")
                .display_order(ORD_TOOLS + 1),
        )
        .subcommand(
            Command::new("update")
                .about("Check for a newer release and update this binary in place")
                .display_order(ORD_TOOLS + 2)
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
                .display_order(ORD_TOOLS + 3)
                .long_about(
                    "Attempt to remove the currently running opencli binary from disk.\n\n\
                     This is best-effort: it works on Unix-like systems where the running \
                     executable can be unlinked, but not on Windows while the binary is in use.\n\n\
                     If opencli was installed through a package manager or symlink, remove \
                     that wrapper separately.",
                ),
        );

    app
}
