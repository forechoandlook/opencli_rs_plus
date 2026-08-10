//! opencli daemon client — connects via TCP JSON-RPC and sends commands.

use anyhow::Result;
use clap::{Parser, Subcommand};
use serde_json::Value;

fn default_addr() -> String {
    "127.0.0.1:10008".to_string()
}

#[derive(Parser)]
#[command(name = "opencli", about = "OpenCLI daemon client")]
struct Cli {
    /// TCP address of daemon (default: 127.0.0.1:10008)
    #[arg(long, global = true)]
    addr: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Manage the daemon process (start/stop/status/logs/autostart)
    Daemon {
        #[command(subcommand)]
        sub: DaemonSubcommand,
    },
    /// Adapter management
    Adapter {
        #[command(subcommand)]
        sub: AdapterSubcommand,
    },
    /// Plugin management
    Plugin {
        #[command(subcommand)]
        sub: PluginSubcommand,
    },
    /// Local key-value store for identity/session hints (`~/.opencli-rs/kv.json`)
    Kv {
        #[command(subcommand)]
        sub: KvSubcommand,
    },
}

#[derive(Subcommand)]
enum KvSubcommand {
    /// Read one key
    Get {
        key: String,
    },
    /// Write one key (value is a string; use --json for raw JSON)
    Set {
        key: String,
        value: String,
        /// Optional TTL: 30d / 24h / 15m / 60s / bare seconds
        #[arg(long)]
        ttl: Option<String>,
        /// Parse value as JSON instead of a plain string
        #[arg(long)]
        json: bool,
    },
    /// List keys (optional prefix filter)
    List {
        #[arg(long)]
        prefix: Option<String>,
    },
    /// Delete one key
    Del {
        key: String,
    },
    /// Clear all keys, or only those with a prefix
    Clear {
        #[arg(long)]
        prefix: Option<String>,
        /// Required when clearing the entire store
        #[arg(long)]
        all: bool,
    },
}

#[derive(Subcommand)]
enum DaemonSubcommand {
    /// Start the daemon in the background (detached, logs to ~/.opencli-rs/daemon.log)
    Start,
    /// Stop the running daemon
    Stop,
    /// Stop and restart the daemon in the background
    Restart,
    /// Show daemon health and process status
    Status,
    /// Show daemon log output
    Logs {
        /// Follow the log file (like tail -f)
        #[arg(short, long)]
        follow: bool,
        /// Number of lines to show from the end
        #[arg(short = 'n', long, default_value = "50")]
        lines: usize,
    },
    /// Show resolved daemon configuration (addr, log path, autostart)
    Config,
    /// Manage boot-time autostart (launchd on macOS, systemd --user on Linux)
    Autostart {
        #[command(subcommand)]
        sub: AutostartSubcommand,
    },
}

#[derive(Subcommand)]
enum AutostartSubcommand {
    /// Install and enable autostart on login
    Install,
    /// Disable and remove autostart
    Uninstall,
    /// Show autostart status
    Status,
}

#[derive(Subcommand)]
enum PluginSubcommand {
    /// Install a plugin
    Install {
        /// Plugin source: user/repo, user/repo/subpath, github:user/repo,
        /// https://..., file:///path, or /local/path
        path: String,
    },
    /// Uninstall a plugin by name
    Uninstall { name: String },
    /// List installed plugins
    List,
    /// Update a plugin (or all plugins if name omitted)
    Update { name: Option<String> },
}

#[derive(Subcommand)]
enum AdapterSubcommand {
    /// List all adapters
    List {
        #[arg(long)]
        include_disabled: bool,
    },
    /// Search adapters
    Search { query: String },
    /// Enable an adapter
    Enable { name: String },
    /// Disable an adapter
    Disable { name: String },
}

// ──────────────────────────────────────────────────────────────────────────────
// Socket client
// ──────────────────────────────────────────────────────────────────────────────

fn socket_request(addr: &str, method: &str, params: Value) -> Result<Value> {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpStream;

    let mut stream = TcpStream::connect(addr)
        .map_err(|e| anyhow::anyhow!("Failed to connect to daemon at {}: {}", addr, e))?;

    let request = serde_json::json!({ "method": method, "params": params });
    let req_str = serde_json::to_string(&request)?;
    stream.write_all(req_str.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;

    let mut reader = BufReader::new(&stream);
    let mut response = String::new();
    reader.read_line(&mut response)?;

    #[derive(serde::Deserialize)]
    struct Resp {
        ok: bool,
        result: Option<Value>,
        error: Option<String>,
        code: Option<i32>,
    }

    let resp: Resp = serde_json::from_str(response.trim())
        .map_err(|e| anyhow::anyhow!("invalid response: {} — raw: {}", e, response))?;

    if resp.ok {
        Ok(resp.result.unwrap_or(Value::Null))
    } else {
        Err(anyhow::anyhow!(
            "daemon error: {} (code {:?})",
            resp.error.unwrap_or_default(),
            resp.code
        ))
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Command handlers
// ──────────────────────────────────────────────────────────────────────────────

fn cmd_status(addr: &str) -> Result<()> {
    let result = socket_request(addr, "daemon.status", serde_json::json!({}))?;
    let status = result
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let chrome = result
        .get("chrome_running")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    println!("Daemon status: {}", status);
    println!("Chrome running: {}", chrome);
    if let Some(adapters) = result.get("adapters") {
        println!(
            "Adapters: {} total, {} enabled",
            adapters.get("total").and_then(|v| v.as_i64()).unwrap_or(0),
            adapters
                .get("enabled")
                .and_then(|v| v.as_i64())
                .unwrap_or(0)
        );
    }
    Ok(())
}

fn cmd_stop(addr: &str) -> Result<()> {
    socket_request(addr, "daemon.stop", serde_json::json!({}))?;
    println!("Daemon stopped");
    Ok(())
}

fn cmd_daemon_start(addr: &str) -> Result<()> {
    use std::fs::OpenOptions;
    use std::process::Stdio;

    if socket_request(addr, "daemon.status", serde_json::json!({})).is_ok() {
        println!("Daemon already running at {}", addr);
        return Ok(());
    }

    let log_path = crate::default_log_path();
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let stdout_file = OpenOptions::new().create(true).append(true).open(&log_path)?;
    let stderr_file = OpenOptions::new().create(true).append(true).open(&log_path)?;

    let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("opencli"));
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("daemon")
        .arg("--addr")
        .arg(addr)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let child = cmd.spawn()?;
    println!(
        "Daemon started in background (pid {}), addr {}, logs: {}",
        child.id(),
        addr,
        log_path.display()
    );
    Ok(())
}

fn cmd_daemon_logs(follow: bool, lines: usize) -> Result<()> {
    let log_path = crate::default_log_path();
    if !log_path.exists() {
        println!("No log file yet at {}", log_path.display());
        return Ok(());
    }
    if follow {
        let status = std::process::Command::new("tail")
            .arg("-f")
            .arg("-n")
            .arg(lines.to_string())
            .arg(&log_path)
            .status()?;
        if !status.success() {
            anyhow::bail!("tail exited with {:?}", status.code());
        }
    } else {
        let content = std::fs::read_to_string(&log_path)?;
        let all_lines: Vec<&str> = content.lines().collect();
        let start = all_lines.len().saturating_sub(lines);
        for line in &all_lines[start..] {
            println!("{}", line);
        }
    }
    Ok(())
}

fn cmd_daemon_config(addr: &str) -> Result<()> {
    let log_path = crate::default_log_path();
    let pid_path = crate::default_pid_path();
    let extension_api_addr = std::env::var("OPENCLI_EXTENSION_API_ADDR")
        .unwrap_or_else(|_| crate::extension_api::DEFAULT_EXTENSION_API_ADDR.to_string());
    let autostart = crate::autostart::status();
    let pid = std::fs::read_to_string(&pid_path).ok();

    println!("Socket addr:        {}", addr);
    println!("Extension API addr: {}", extension_api_addr);
    println!("Log path:           {}", log_path.display());
    println!(
        "PID file:           {} ({})",
        pid_path.display(),
        pid.as_deref().unwrap_or("not running")
    );
    println!(
        "Autostart:          {} — file {} — installed: {} — loaded: {}",
        autostart.platform,
        autostart.file_path.display(),
        autostart.installed,
        autostart.loaded
    );
    Ok(())
}

fn cmd_autostart_install(addr: &str) -> Result<()> {
    crate::autostart::install(addr)?;
    let status = crate::autostart::status();
    println!(
        "Autostart installed via {} ({})",
        status.platform,
        status.file_path.display()
    );
    Ok(())
}

fn cmd_autostart_uninstall() -> Result<()> {
    crate::autostart::uninstall()?;
    println!("Autostart removed");
    Ok(())
}

fn cmd_autostart_status() -> Result<()> {
    let status = crate::autostart::status();
    println!("Platform:  {}", status.platform);
    println!("File:      {}", status.file_path.display());
    println!("Installed: {}", status.installed);
    println!("Loaded:    {}", status.loaded);
    Ok(())
}

fn cmd_adapter_list(addr: &str, include_disabled: bool) -> Result<()> {
    let result = socket_request(
        addr,
        "adapter.list",
        serde_json::json!({
            "include_disabled": include_disabled,
        }),
    )?;
    let adapters = result
        .get("adapters")
        .and_then(|v| v.as_array())
        .map_or(&[] as &[_], |v| v.as_slice());
    let count = result.get("count").and_then(|v| v.as_i64()).unwrap_or(0);
    if adapters.is_empty() {
        println!("No adapters found.");
        return Ok(());
    }
    println!(
        "{:30} {:10} {:12} Description",
        "Name", "Enabled", "Browser"
    );
    println!("{}", "-".repeat(80));
    for entry in adapters {
        let name = entry
            .get("full_name")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let enabled = entry
            .get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let browser = entry
            .get("browser")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let desc = entry
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        println!(
            "{:30} {:10} {:12} {}",
            name,
            if enabled { "yes" } else { "no" },
            if browser { "yes" } else { "no" },
            desc.chars().take(40).collect::<String>()
        );
    }
    println!("\n{} adapters total", count);
    Ok(())
}

fn cmd_adapter_search(addr: &str, query: &str) -> Result<()> {
    // Prefer daemon registry (plugins + disable state); fall back to local scan.
    match socket_request(
        addr,
        "adapter.search",
        serde_json::json!({ "query": query }),
    ) {
        Ok(result) => {
            let adapters = result
                .get("adapters")
                .and_then(|v| v.as_array())
                .map_or(&[] as &[_], |v| v.as_slice());
            if adapters.is_empty() {
                println!("No adapters found matching '{}'.", query);
                return Ok(());
            }
            for entry in adapters {
                print_adapter_hit_json(entry);
            }
            println!(
                "\n{} match(es) for '{}'",
                result.get("count").and_then(|v| v.as_i64()).unwrap_or(0),
                query
            );
        }
        Err(e) if e.to_string().contains("Failed to connect") => {
            eprintln!("(daemon not running — local adapters only)");
            cmd_adapter_search_local(query)?;
        }
        Err(e) => return Err(e),
    }
    Ok(())
}

/// Local fallback: substring match on name / site / description / domain.
fn cmd_adapter_search_local(query: &str) -> Result<()> {
    use opencli_rs_core::Registry;
    use opencli_rs_discovery::discover_adapters;

    let mut registry = Registry::new();
    let _ = discover_adapters(&mut registry);

    let q = query.to_lowercase();
    let mut hits = Vec::new();
    for site in registry.list_sites() {
        for cmd in registry.list_commands(site) {
            let domain = cmd.domain.as_deref().unwrap_or("");
            let haystack = format!(
                "{} {} {} {}",
                cmd.full_name(),
                site,
                cmd.description,
                domain
            )
            .to_lowercase();
            if q.is_empty() || haystack.contains(&q) {
                hits.push(cmd.clone());
            }
        }
    }

    if hits.is_empty() {
        println!("No adapters found matching '{}'.", query);
        return Ok(());
    }

    for cmd in &hits {
        print_adapter_hit_cmd(cmd);
    }
    println!("\n{} match(es) for '{}'", hits.len(), query);
    Ok(())
}

fn print_adapter_hit_json(entry: &Value) {
    let name = entry
        .get("full_name")
        .and_then(|v| v.as_str())
        .unwrap_or("?");
    let browser = entry
        .get("browser")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let desc = entry
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    println!(
        "\n{}  (browser: {})",
        name,
        if browser { "yes" } else { "no" }
    );
    if !desc.is_empty() {
        println!("  {}", desc);
    }
    println!("  用法: {}", usage_from_entry_json(entry));
}

fn print_adapter_hit_cmd(cmd: &opencli_rs_core::CliCommand) {
    println!(
        "\n{}  (browser: {})",
        cmd.full_name(),
        if cmd.needs_browser() { "yes" } else { "no" }
    );
    if !cmd.description.is_empty() {
        println!("  {}", cmd.description);
    }
    println!("  用法: {}", adapter_usage_line(cmd));
}

fn usage_from_entry_json(entry: &Value) -> String {
    let name = entry
        .get("full_name")
        .and_then(|v| v.as_str())
        .unwrap_or("?");
    let mut parts = vec![format!("opencli {}", name)];
    let args = entry
        .get("args")
        .and_then(|v| v.as_array())
        .map(|a| a.as_slice())
        .unwrap_or(&[]);
    for a in args {
        let n = a.get("name").and_then(|v| v.as_str()).unwrap_or("arg");
        let positional = a
            .get("positional")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let required = a
            .get("required")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !positional {
            continue;
        }
        parts.push(if required {
            format!("<{}>", n)
        } else {
            format!("[<{}>]", n)
        });
    }
    for a in args {
        let n = a.get("name").and_then(|v| v.as_str()).unwrap_or("arg");
        let positional = a
            .get("positional")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let required = a
            .get("required")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if positional {
            continue;
        }
        if required {
            parts.push(format!("--{} <value>", n));
        } else if let Some(d) = a.get("default").and_then(|v| {
            if v.is_string() {
                v.as_str().map(|s| s.to_string())
            } else if v.is_null() {
                None
            } else {
                Some(v.to_string())
            }
        }) {
            parts.push(format!("[--{}={}]", n, d));
        } else {
            parts.push(format!("[--{} <value>]", n));
        }
    }
    parts.join(" ")
}

fn cmd_adapter_enable(addr: &str, name: &str) -> Result<()> {
    let name = opencli_rs_core::AdapterSettings::normalize_name(name);
    match socket_request(addr, "adapter.enable", serde_json::json!({ "name": name })) {
        Ok(result) => {
            let enabled = result
                .get("enabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if enabled {
                println!("Adapter '{}' enabled", name);
            } else {
                println!("Adapter '{}' was not on the disable list", name);
            }
        }
        Err(e) if e.to_string().contains("Failed to connect") => {
            let mut settings = opencli_rs_core::AdapterSettings::load();
            let changed = settings.enable(&name).map_err(|e| anyhow::anyhow!(e))?;
            if changed {
                println!(
                    "Adapter '{}' enabled (wrote {})",
                    name,
                    opencli_rs_core::AdapterSettings::path().display()
                );
            } else {
                println!("Adapter '{}' was not on the disable list", name);
            }
        }
        Err(e) => return Err(e),
    }
    Ok(())
}

fn cmd_adapter_disable(addr: &str, name: &str) -> Result<()> {
    let name = opencli_rs_core::AdapterSettings::normalize_name(name);
    match socket_request(addr, "adapter.disable", serde_json::json!({ "name": name })) {
        Ok(_) => {
            println!(
                "Adapter '{}' disabled\n  (hidden from help/search; direct runs blocked)",
                name
            );
            if !name.contains(' ') {
                println!("  note: bare site name disables the whole '{}' family", name);
            }
        }
        Err(e) if e.to_string().contains("Failed to connect") => {
            let mut settings = opencli_rs_core::AdapterSettings::load();
            settings.disable(&name).map_err(|e| anyhow::anyhow!(e))?;
            println!(
                "Adapter '{}' disabled (wrote {})\n  (hidden from help/search; direct runs blocked)",
                name,
                opencli_rs_core::AdapterSettings::path().display()
            );
            if !name.contains(' ') {
                println!("  note: bare site name disables the whole '{}' family", name);
            }
        }
        Err(e) => return Err(e),
    }
    Ok(())
}

fn cmd_plugin_install(addr: &str, path: &str) -> Result<()> {
    // Expand bare "user/repo" and "user/repo/subpath" → "github:user/repo[/subpath]"
    let source = if !path.contains(':') && !path.starts_with('/') {
        format!("github:{}", path)
    } else {
        path.to_string()
    };
    let result = socket_request(
        addr,
        "plugin.install",
        serde_json::json!({ "path": source }),
    )?;
    let plugin = &result["plugin"];
    println!(
        "Installed plugin '{}' ({})",
        plugin["name"].as_str().unwrap_or("?"),
        plugin["source"].as_str().unwrap_or("?"),
    );
    if let Some(desc) = plugin["description"].as_str().filter(|s| !s.is_empty()) {
        println!("  {}", desc);
    }
    Ok(())
}

fn cmd_plugin_uninstall(addr: &str, name: &str) -> Result<()> {
    socket_request(
        addr,
        "plugin.uninstall",
        serde_json::json!({ "name": name }),
    )?;
    println!("Uninstalled plugin '{}'", name);
    Ok(())
}

fn cmd_plugin_list(addr: &str) -> Result<()> {
    let result = socket_request(addr, "plugin.list", serde_json::json!({}))?;
    let plugins = result
        .get("plugins")
        .and_then(|v| v.as_array())
        .map_or(&[] as &[_], |v| v.as_slice());
    if plugins.is_empty() {
        println!("No plugins installed.");
        return Ok(());
    }
    println!("{:25} {:10} Source", "Name", "Version");
    println!("{}", "-".repeat(80));
    for p in plugins {
        println!(
            "{:25} {:10} {}",
            p["name"].as_str().unwrap_or("?"),
            p["version"].as_str().unwrap_or("-"),
            p["source"].as_str().unwrap_or("?"),
        );
    }
    println!("\n{} plugin(s)", plugins.len());
    Ok(())
}

fn cmd_plugin_update(addr: &str, name: Option<&str>) -> Result<()> {
    let params = match name {
        Some(n) => serde_json::json!({ "name": n }),
        None => serde_json::json!({}),
    };
    let result = socket_request(addr, "plugin.update", params)?;
    let updated = result
        .get("updated")
        .and_then(|v| v.as_array())
        .map_or(vec![], |v| v.iter().filter_map(|s| s.as_str()).collect());
    if updated.is_empty() {
        println!("Nothing to update.");
    } else {
        println!("Updated: {}", updated.join(", "));
    }
    if let Some(errors) = result.get("errors").and_then(|v| v.as_array()) {
        for e in errors {
            eprintln!(
                "  error: {} — {}",
                e["plugin"].as_str().unwrap_or("?"),
                e["error"].as_str().unwrap_or("?")
            );
        }
    }
    Ok(())
}

// ──────────────────────────────────────────────────────────────────────────────
// Main
// ──────────────────────────────────────────────────────────────────────────────

pub fn run() -> Result<()> {
    let args = Cli::parse();
    let addr = args.addr.unwrap_or_else(default_addr);

    match args.command {
        Command::Daemon { sub } => match sub {
            DaemonSubcommand::Start => cmd_daemon_start(&addr)?,
            DaemonSubcommand::Stop => cmd_stop(&addr)?,
            DaemonSubcommand::Restart => {
                let _ = cmd_stop(&addr);
                std::thread::sleep(std::time::Duration::from_secs(1));
                cmd_daemon_start(&addr)?
            }
            DaemonSubcommand::Status => cmd_status(&addr)?,
            DaemonSubcommand::Logs { follow, lines } => cmd_daemon_logs(follow, lines)?,
            DaemonSubcommand::Config => cmd_daemon_config(&addr)?,
            DaemonSubcommand::Autostart { sub } => match sub {
                AutostartSubcommand::Install => cmd_autostart_install(&addr)?,
                AutostartSubcommand::Uninstall => cmd_autostart_uninstall()?,
                AutostartSubcommand::Status => cmd_autostart_status()?,
            },
        },

        Command::Adapter { sub } => match sub {
            AdapterSubcommand::List { include_disabled } => {
                cmd_adapter_list(&addr, include_disabled)?;
            }
            AdapterSubcommand::Search { query } => cmd_adapter_search(&addr, &query)?,
            AdapterSubcommand::Enable { name } => cmd_adapter_enable(&addr, &name)?,
            AdapterSubcommand::Disable { name } => cmd_adapter_disable(&addr, &name)?,
        },

        Command::Plugin { sub } => match sub {
            PluginSubcommand::Install { path } => cmd_plugin_install(&addr, &path)?,
            PluginSubcommand::Uninstall { name } => cmd_plugin_uninstall(&addr, &name)?,
            PluginSubcommand::List => cmd_plugin_list(&addr)?,
            PluginSubcommand::Update { name } => cmd_plugin_update(&addr, name.as_deref())?,
        },

        Command::Kv { sub } => cmd_kv(sub)?,
    }

    Ok(())
}

fn cmd_kv(sub: KvSubcommand) -> Result<()> {
    use opencli_rs_core::kv;
    let map_err = |e: String| anyhow::anyhow!(e);
    match sub {
        KvSubcommand::Get { key } => match kv::get(&key).map_err(map_err)? {
            Some(v) => match v {
                Value::String(s) => println!("{s}"),
                other => println!("{}", serde_json::to_string_pretty(&other)?),
            },
            None => {
                eprintln!("(missing) {key}");
                std::process::exit(1);
            }
        },
        KvSubcommand::Set {
            key,
            value,
            ttl,
            json,
        } => {
            let parsed = if json {
                serde_json::from_str(&value)
                    .map_err(|e| anyhow::anyhow!("--json value is not valid JSON: {e}"))?
            } else {
                Value::String(value)
            };
            kv::set(&key, parsed, ttl.as_deref()).map_err(map_err)?;
            println!("ok {key}");
        }
        KvSubcommand::List { prefix } => {
            let entries = kv::list(prefix.as_deref()).map_err(map_err)?;
            if entries.is_empty() {
                println!("(empty)");
                return Ok(());
            }
            for (k, e) in entries {
                let val = match &e.value {
                    Value::String(s) => s.clone(),
                    other => serde_json::to_string(other).unwrap_or_default(),
                };
                let exp = e
                    .expires_at
                    .map(|t| format!(" expires_at={t}"))
                    .unwrap_or_default();
                println!("{k} = {val}  (updated_at={}{exp})", e.updated_at);
            }
        }
        KvSubcommand::Del { key } => {
            if kv::del(&key).map_err(map_err)? {
                println!("deleted {key}");
            } else {
                println!("(missing) {key}");
            }
        }
        KvSubcommand::Clear { prefix, all } => {
            if prefix.is_none() && !all {
                anyhow::bail!("Refusing to clear entire KV store without --all (or pass --prefix)");
            }
            let n = kv::clear(prefix.as_deref()).map_err(map_err)?;
            println!("cleared {n} key(s)");
        }
    }
    Ok(())
}

/// Build a ready-to-copy invocation string from an adapter's declared args.
fn adapter_usage_line(cmd: &opencli_rs_core::CliCommand) -> String {
    let mut parts = vec![format!("opencli {}", cmd.full_name())];
    for a in &cmd.args {
        if !a.positional {
            continue;
        }
        parts.push(if a.required {
            format!("<{}>", a.name)
        } else {
            format!("[<{}>]", a.name)
        });
    }
    for a in &cmd.args {
        if a.positional {
            continue;
        }
        if a.required {
            parts.push(format!("--{} <{}>", a.name, arg_type_hint(a)));
        }
    }
    for a in &cmd.args {
        if a.positional || a.required {
            continue;
        }
        match &a.default {
            Some(v) => parts.push(format!("[--{}={}]", a.name, v)),
            None => parts.push(format!("[--{} <{}>]", a.name, arg_type_hint(a))),
        }
    }
    parts.join(" ")
}

fn arg_type_hint(a: &opencli_rs_core::ArgDef) -> &'static str {
    use opencli_rs_core::ArgType;
    match a.arg_type {
        ArgType::Int => "int",
        ArgType::Number => "number",
        ArgType::Bool | ArgType::Boolean => "bool",
        ArgType::Str => "str",
    }
}
