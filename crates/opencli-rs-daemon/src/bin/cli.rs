//! opencli-cli — client for the opencli-daemon scheduler.
//! Connects to the daemon via TCP JSON-RPC and sends commands.

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use clap::{Parser, Subcommand};
#[path = "../tools.rs"]
mod tools;
use serde_json::Value;
use std::path::PathBuf;
use tools::{find_by_name, load_tools, search, summary};

fn default_addr() -> String {
    "127.0.0.1:10008".to_string()
}

#[derive(Parser)]
#[command(name = "opencli-cli", about = "OpenCLI daemon client")]
struct Cli {
    /// TCP address of daemon (default: 127.0.0.1:10008)
    #[arg(long, global = true)]
    addr: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check daemon status
    Status,
    /// Stop the running daemon
    Stop,
    /// Restart the daemon
    Restart {
        #[arg(long, default_value = "10")]
        poll_interval: u64,
        #[arg(long)]
        db: Option<PathBuf>,
    },
    /// Job management
    Job {
        #[command(subcommand)]
        sub: JobSubcommand,
    },
    /// Adapter management
    Adapter {
        #[command(subcommand)]
        sub: AdapterSubcommand,
    },
    /// Tool knowledge base
    Tools {
        #[command(subcommand)]
        sub: ToolsSubcommand,
    },
    /// Plugin management
    Plugin {
        #[command(subcommand)]
        sub: PluginSubcommand,
    },
    /// Send a raw socket command (for debugging)
    Socket {
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
}

#[derive(Subcommand)]
enum ToolsSubcommand {
    /// Search for tools by keyword
    Search { query: String },
    /// List all tools
    List,
    /// Show tool details
    Info { name: String },
    /// Show all tool names and short descriptions
    Summary,
}

#[derive(Subcommand)]
enum JobSubcommand {
    /// Add a new job
    Add {
        adapter: String,
        #[arg(short, long)]
        run_at: Option<String>,
        #[arg(short = 'd', long)]
        delay: Option<i64>,
        #[arg(short, long)]
        interval: Option<i64>,
        #[arg(short, long)]
        args: Option<String>,
    },
    /// List jobs
    List {
        #[arg(short, long)]
        status: Option<String>,
        #[arg(short, long, default_value = "50")]
        limit: usize,
    },
    /// Show job details
    Show { id: String },
    /// Cancel a job
    Cancel { id: String },
    /// Delete a job
    Delete { id: String },
    /// Trigger due jobs immediately
    Run,
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
        #[arg(long)]
        include_hidden: bool,
    },
    /// Search adapters
    Search { query: String },
    /// Enable an adapter
    Enable { name: String },
    /// Disable an adapter
    Disable { name: String },
    /// Sync adapters from a folder
    Sync {
        #[arg(short, long)]
        folder: Option<PathBuf>,
    },
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

fn parse_datetime(s: &str) -> Result<DateTime<Utc>> {
    if s == "now" {
        return Ok(Utc::now());
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc));
    }
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return Ok(dt.and_utc());
    }
    if let Ok(dt) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Ok(dt.and_hms_opt(0, 0, 0).unwrap().and_utc());
    }
    anyhow::bail!("Invalid date format: {}. Use ISO8601 or 'now'", s)
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
    if let Some(jobs) = result.get("jobs") {
        println!(
            "Jobs: {} pending, {} running",
            jobs.get("pending").and_then(|v| v.as_i64()).unwrap_or(0),
            jobs.get("running").and_then(|v| v.as_i64()).unwrap_or(0)
        );
    }
    Ok(())
}

fn cmd_stop(addr: &str) -> Result<()> {
    socket_request(addr, "daemon.stop", serde_json::json!({}))?;
    println!("Daemon stopped");
    Ok(())
}

fn cmd_restart(addr: &str, poll_interval: u64, db: Option<PathBuf>) -> Result<()> {
    let _ = cmd_stop(addr);
    std::thread::sleep(std::time::Duration::from_secs(1));
    let mut child = std::process::Command::new("opencli-daemon");
    child.arg("--poll-interval").arg(poll_interval.to_string());
    child.arg("--addr").arg(addr);
    if let Some(db) = db {
        child.arg("--db").arg(db);
    }
    child.spawn()?;
    println!("Daemon restarted");
    Ok(())
}

fn cmd_adapter_list(addr: &str, include_disabled: bool, include_hidden: bool) -> Result<()> {
    let result = socket_request(
        addr,
        "adapter.list",
        serde_json::json!({
            "include_disabled": include_disabled,
            "include_hidden": include_hidden,
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
        "{:30} {:10} {:12} {}",
        "Name", "Enabled", "Browser", "Description"
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
    let result = socket_request(
        addr,
        "adapter.search",
        serde_json::json!({ "query": query }),
    )?;
    let adapters = result
        .get("adapters")
        .and_then(|v| v.as_array())
        .map_or(&[] as &[_], |v| v.as_slice());
    let count = result.get("count").and_then(|v| v.as_i64()).unwrap_or(0);
    if adapters.is_empty() {
        println!("No adapters found matching '{}'.", query);
        return Ok(());
    }
    println!("{:30} {:12} {}", "Name", "Browser", "Description");
    println!("{}", "-".repeat(70));
    for entry in adapters {
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
            "{:30} {:12} {}",
            name,
            if browser { "yes" } else { "no" },
            desc.chars().take(35).collect::<String>()
        );
    }
    println!("\n{} results for '{}'", count, query);
    Ok(())
}

fn cmd_adapter_enable(addr: &str, name: &str) -> Result<()> {
    let result = socket_request(addr, "adapter.enable", serde_json::json!({ "name": name }))?;
    let enabled = result
        .get("enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    println!(
        "{}",
        if enabled {
            format!("Adapter '{}' enabled", name)
        } else {
            format!("Failed to enable '{}'", name)
        }
    );
    Ok(())
}

fn cmd_adapter_disable(addr: &str, name: &str) -> Result<()> {
    let result = socket_request(addr, "adapter.disable", serde_json::json!({ "name": name }))?;
    let enabled = result
        .get("enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    println!(
        "{}",
        if !enabled {
            format!("Adapter '{}' disabled", name)
        } else {
            format!("Failed to disable '{}'", name)
        }
    );
    Ok(())
}

fn cmd_adapter_sync(addr: &str, folder: Option<PathBuf>) -> Result<()> {
    let result = socket_request(
        addr,
        "adapter.sync",
        serde_json::json!({
            "folder": folder.map(|p| p.display().to_string()),
        }),
    )?;
    let synced = result.get("synced").and_then(|v| v.as_i64()).unwrap_or(0);
    let folder_str = result.get("folder").and_then(|v| v.as_str()).unwrap_or("?");
    println!("Synced {} adapters from '{}'", synced, folder_str);
    Ok(())
}

fn cmd_job_add(
    addr: &str,
    adapter: &str,
    run_at: Option<String>,
    delay: Option<i64>,
    interval: Option<i64>,
    args_json: Option<String>,
) -> Result<()> {
    let run_at_dt = match (run_at.as_deref(), delay) {
        (Some(s), _) => parse_datetime(s)?,
        (None, Some(d)) => Utc::now() + Duration::seconds(d),
        (None, None) => Utc::now(),
    };
    let args_val: Option<Value> = args_json
        .as_ref()
        .and_then(|s| serde_json::from_str(s).ok());
    let result = socket_request(
        addr,
        "job.add",
        serde_json::json!({
            "adapter": adapter,
            "args": args_val,
            "run_at": run_at_dt.to_rfc3339(),
            "interval_seconds": interval,
        }),
    )?;
    let job = &result["job"];
    println!("Job created: {}", job["id"].as_str().unwrap_or("?"));
    println!(
        "   Adapter: {}  |  Run at: {}  |  Status: {}",
        job["adapter"].as_str().unwrap_or("?"),
        job["run_at"].as_str().unwrap_or("?"),
        job["status"].as_str().unwrap_or("?")
    );
    Ok(())
}

fn cmd_job_list(addr: &str, status: Option<String>, limit: usize) -> Result<()> {
    let result = socket_request(
        addr,
        "job.list",
        serde_json::json!({
            "status": status,
            "limit": limit,
        }),
    )?;
    let jobs = result
        .get("jobs")
        .and_then(|v| v.as_array())
        .map_or(&[] as &[_], |v| v.as_slice());
    if jobs.is_empty() {
        println!("No jobs found.");
        return Ok(());
    }
    println!(
        "{:40} {:20} {:12} {:20}",
        "ID", "Adapter", "Status", "Run At"
    );
    println!("{}", "-".repeat(95));
    for job in jobs {
        let id = job["id"].as_str().unwrap_or("?");
        println!(
            "{:40} {:20} {:12} {}",
            &id[..8.min(id.len())],
            job["adapter"].as_str().unwrap_or("?"),
            job["status"].as_str().unwrap_or("?"),
            job["run_at"].as_str().unwrap_or("?")
        );
    }
    println!("\n{} jobs total", jobs.len());
    Ok(())
}

fn cmd_job_show(addr: &str, id: &str) -> Result<()> {
    let result = socket_request(addr, "job.show", serde_json::json!({ "id": id }))?;
    let job = &result["job"];
    println!("ID:       {}", job["id"].as_str().unwrap_or("?"));
    println!("Adapter:  {}", job["adapter"].as_str().unwrap_or("?"));
    println!("Status:   {}", job["status"].as_str().unwrap_or("?"));
    println!("Run at:   {}", job["run_at"].as_str().unwrap_or("?"));
    if let Some(i) = job["interval_seconds"].as_i64() {
        if i > 0 {
            println!("Interval: {}s", i);
        }
    }
    if let Some(args) = job.get("args").filter(|v| !v.is_null()) {
        println!(
            "Args:     {}",
            serde_json::to_string_pretty(args).unwrap_or_default()
        );
    }
    if let Some(r) = job["result"].as_str() {
        println!("Result:   {}", r.chars().take(200).collect::<String>());
    }
    if let Some(e) = job["error"].as_str() {
        println!("Error:    {}", e);
    }
    Ok(())
}

fn cmd_job_cancel(addr: &str, id: &str) -> Result<()> {
    socket_request(addr, "job.cancel", serde_json::json!({ "id": id }))?;
    println!("Cancelled: {}", id);
    Ok(())
}

fn cmd_job_delete(addr: &str, id: &str) -> Result<()> {
    socket_request(addr, "job.delete", serde_json::json!({ "id": id }))?;
    println!("Deleted: {}", id);
    Ok(())
}

fn cmd_tools_search(query: &str) -> Result<()> {
    let tools = load_tools();
    let results = search(query, &tools);
    if results.is_empty() {
        println!("No tools found for '{}'.", query);
        return Ok(());
    }
    println!(
        "{:25} {:20} {:10} {}",
        "Name", "Binary", "Installed", "Description"
    );
    println!("{}", "-".repeat(85));
    for t in results {
        println!(
            "{:25} {:20} {:10} {}",
            t.name,
            t.binary,
            if t.is_installed() { "yes" } else { "no" },
            t.description.chars().take(35).collect::<String>()
        );
    }
    Ok(())
}

fn cmd_tools_list() -> Result<()> {
    let tools = load_tools();
    if tools.is_empty() {
        println!("No tools found. Add .md files to ~/.opencli-rs/tools/");
        return Ok(());
    }
    println!(
        "{:25} {:20} {:10} {}",
        "Name", "Binary", "Installed", "Description"
    );
    println!("{}", "-".repeat(85));
    for t in &tools {
        println!(
            "{:25} {:20} {:10} {}",
            t.name,
            t.binary,
            if t.is_installed() { "yes" } else { "no" },
            t.description.chars().take(35).collect::<String>()
        );
    }
    Ok(())
}

fn cmd_tools_info(name: &str) -> Result<()> {
    let tools = load_tools();
    match find_by_name(name, &tools) {
        None => println!("Tool '{}' not found.", name),
        Some(t) => {
            println!("Name:        {}", t.name);
            println!("Binary:      {}", t.binary);
            println!(
                "Installed:   {}",
                if t.is_installed() { "yes" } else { "no" }
            );
            if let Some(hp) = &t.homepage {
                println!("Homepage:    {}", hp);
            }
            if !t.description.is_empty() {
                println!("Description: {}", t.description);
            }
            if !t.tags.is_empty() {
                println!("Tags:        {}", t.tags.join(", "));
            }
            if let Some(cmd) = t.install_cmd() {
                println!("Install:     {}", cmd);
            }
            if !t.body.trim().is_empty() {
                println!("\n{}", t.body.trim());
            }
        }
    }
    Ok(())
}

fn cmd_tools_summary() -> Result<()> {
    let tools = load_tools();
    if tools.is_empty() {
        println!("No tools found. Add .md files to ~/.opencli-rs/tools/");
        return Ok(());
    }
    let items = summary(&tools);
    println!("{:25} {:10} {}", "Name", "Installed", "Description");
    println!("{}", "-".repeat(70));
    for s in items {
        println!(
            "{:25} {:10} {}",
            s.name,
            if s.installed { "yes" } else { "no" },
            s.description.chars().take(35).collect::<String>()
        );
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
    println!("{:25} {:10} {}", "Name", "Version", "Source");
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

fn cmd_job_run(addr: &str) -> Result<()> {
    socket_request(addr, "job.run", serde_json::json!({}))?;
    println!("Due jobs triggered");
    Ok(())
}

// ──────────────────────────────────────────────────────────────────────────────
// Main
// ──────────────────────────────────────────────────────────────────────────────

fn main() -> Result<()> {
    let args = Cli::parse();
    let addr = args.addr.unwrap_or_else(default_addr);

    match args.command {
        Command::Status => cmd_status(&addr)?,
        Command::Stop => cmd_stop(&addr)?,
        Command::Restart { poll_interval, db } => cmd_restart(&addr, poll_interval, db)?,

        Command::Job { sub } => match sub {
            JobSubcommand::Add {
                adapter,
                run_at,
                delay,
                interval,
                args: args_json,
            } => {
                cmd_job_add(&addr, &adapter, run_at, delay, interval, args_json)?;
            }
            JobSubcommand::List { status, limit } => cmd_job_list(&addr, status, limit)?,
            JobSubcommand::Show { id } => cmd_job_show(&addr, &id)?,
            JobSubcommand::Cancel { id } => cmd_job_cancel(&addr, &id)?,
            JobSubcommand::Delete { id } => cmd_job_delete(&addr, &id)?,
            JobSubcommand::Run => cmd_job_run(&addr)?,
        },

        Command::Adapter { sub } => match sub {
            AdapterSubcommand::List {
                include_disabled,
                include_hidden,
            } => {
                cmd_adapter_list(&addr, include_disabled, include_hidden)?;
            }
            AdapterSubcommand::Search { query } => cmd_adapter_search(&addr, &query)?,
            AdapterSubcommand::Enable { name } => cmd_adapter_enable(&addr, &name)?,
            AdapterSubcommand::Disable { name } => cmd_adapter_disable(&addr, &name)?,
            AdapterSubcommand::Sync { folder } => cmd_adapter_sync(&addr, folder)?,
        },

        Command::Tools { sub } => match sub {
            ToolsSubcommand::Search { query } => cmd_tools_search(&query)?,
            ToolsSubcommand::List => cmd_tools_list()?,
            ToolsSubcommand::Info { name } => cmd_tools_info(&name)?,
            ToolsSubcommand::Summary => cmd_tools_summary()?,
        },

        Command::Plugin { sub } => match sub {
            PluginSubcommand::Install { path } => cmd_plugin_install(&addr, &path)?,
            PluginSubcommand::Uninstall { name } => cmd_plugin_uninstall(&addr, &name)?,
            PluginSubcommand::List => cmd_plugin_list(&addr)?,
            PluginSubcommand::Update { name } => cmd_plugin_update(&addr, name.as_deref())?,
        },

        Command::Socket { args: raw_args } => {
            if raw_args.is_empty() {
                anyhow::bail!("Usage: socket <method> [params_json]");
            }
            let method = &raw_args[0];
            let params: Value = raw_args
                .get(1)
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or(serde_json::json!({}));
            let result = socket_request(&addr, method, params)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
    }

    Ok(())
}
