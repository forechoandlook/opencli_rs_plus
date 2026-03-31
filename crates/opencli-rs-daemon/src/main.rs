mod adapter_manager;
mod scheduler;
mod socket;
mod store;

use adapter_manager::AdapterManager;
use anyhow::Result;
use clap::{Parser, Subcommand};
use chrono::{DateTime, Duration, Utc};
use scheduler::Scheduler;
use serde_json::Value;
use socket::{serve, SocketState};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::signal;
use tracing::{error, info, Level};
use tracing_subscriber::FmtSubscriber;

use crate::store::{JobStatus, JobStore};

fn default_db_path() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".opencli-rs").join("jobs.db"))
        .unwrap_or_else(|| PathBuf::from("jobs.db"))
}

fn default_socket_path() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".opencli-rs").join("daemon.sock"))
        .unwrap_or_else(|| PathBuf::from("daemon.sock"))
}

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Start the daemon (socket server + scheduler)
    Start {
        /// Polling interval in seconds
        #[arg(long, default_value = "10")]
        poll_interval: u64,
        /// Database path (default: ~/.opencli-rs/jobs.db)
        #[arg(long)]
        db: Option<PathBuf>,
        /// Socket path (default: ~/.opencli-rs/daemon.sock)
        #[arg(long)]
        socket: Option<PathBuf>,
        /// Skip adapter loading on startup
        #[arg(long)]
        no_load_adapters: bool,
    },
    /// Stop the running daemon
    Stop {
        /// Socket path
        #[arg(long)]
        socket: Option<PathBuf>,
    },
    /// Check daemon status (via socket)
    Status {
        /// Socket path
        #[arg(long)]
        socket: Option<PathBuf>,
    },
    /// Restart the daemon
    Restart {
        #[arg(long, default_value = "10")]
        poll_interval: u64,
        #[arg(long)]
        db: Option<PathBuf>,
        #[arg(long)]
        socket: Option<PathBuf>,
    },
    /// Run the scheduler inline (for testing or cron-driven execution)
    Job {
        #[command(subcommand)]
        sub: JobSubcommand,
    },
    /// Adapter management
    Adapter {
        #[command(subcommand)]
        sub: AdapterSubcommand,
    },
    /// Send a raw socket command (for debugging / scripting)
    Socket {
        /// JSON-RPC request body
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
}

#[derive(Subcommand)]
enum JobSubcommand {
    /// Add a new job
    Add {
        /// Adapter name in format "site command" (e.g. "zhihu collections")
        adapter: String,
        /// Run at time (ISO8601, e.g. "2026-03-31T10:00:00Z") or "now"
        #[arg(short, long)]
        run_at: Option<String>,
        /// Run after N seconds (e.g. --delay 300)
        #[arg(short = 'd', long)]
        delay: Option<i64>,
        /// Repeat every N seconds (e.g. --interval 3600)
        #[arg(short, long)]
        interval: Option<i64>,
        /// JSON arguments for the adapter (e.g. --args '{"url":"..."}')
        #[arg(short, long)]
        args: Option<String>,
    },
    /// List jobs
    List {
        /// Filter by status: pending, running, done, failed, cancelled
        #[arg(short, long)]
        status: Option<String>,
        /// Max results
        #[arg(short, long, default_value = "50")]
        limit: usize,
    },
    /// Show job details
    Show {
        id: String,
    },
    /// Cancel a job
    Cancel {
        id: String,
    },
    /// Delete a job
    Delete {
        id: String,
    },
    /// Run due jobs immediately (for cron-driven mode)
    Run,
}

#[derive(Subcommand)]
enum AdapterSubcommand {
    /// List all adapters
    List {
        /// Include disabled adapters
        #[arg(long)]
        include_disabled: bool,
        /// Include hidden adapters
        #[arg(long)]
        include_hidden: bool,
    },
    /// Search adapters by name or description
    Search {
        query: String,
    },
    /// Enable an adapter
    Enable {
        /// Full adapter name "site command"
        name: String,
    },
    /// Disable an adapter
    Disable {
        /// Full adapter name "site command"
        name: String,
    },
    /// Sync adapters from a folder
    Sync {
        /// Folder path (default: ~/.opencli-rs/adapters)
        #[arg(short, long)]
        folder: Option<PathBuf>,
    },
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

fn open_store(db: Option<PathBuf>) -> Result<JobStore> {
    let db_path = db.unwrap_or_else(default_db_path);
    JobStore::new(db_path).map_err(|e| anyhow::anyhow!("{}", e))
}

// ──────────────────────────────────────────────────────────────────────────────
// Daemon process
// ──────────────────────────────────────────────────────────────────────────────

async fn run_daemon(
    poll_interval: u64,
    db: Option<PathBuf>,
    socket_path: PathBuf,
    _no_load_adapters: bool,
) -> Result<()> {
    // Remove stale socket
    if socket_path.exists() {
        tokio::fs::remove_file(&socket_path).await?;
    }

    let job_store = Arc::new(open_store(db)?);
    let adapter_manager = Arc::new(AdapterManager::new().await?);
    let scheduler = Arc::new(Scheduler::new(
        Arc::clone(&job_store),
        Arc::clone(&adapter_manager),
        poll_interval,
    ));

    // Start scheduler loop
    let sched = Arc::clone(&scheduler);
    let sched_handle = tokio::spawn(async move {
        sched.run_loop().await;
    });

    // Start socket server
    let socket_state = Arc::new(SocketState {
        adapter_manager,
        scheduler,
        job_store,
    });

    let socket_path_clone = socket_path.clone();
    let socket_handle = tokio::spawn(async move {
        if let Err(e) = serve(socket_path_clone, socket_state).await {
            error!(error = %e, "Socket server error");
        }
    });

    info!(
        socket = %socket_path.display(),
        poll_interval = poll_interval,
        "Daemon started"
    );

    // Wait for shutdown signal
    signal::ctrl_c().await?;
    info!("Shutting down daemon");
    sched_handle.abort();
    socket_handle.abort();
    tokio::fs::remove_file(&socket_path).await.ok();
    Ok(())
}

// ──────────────────────────────────────────────────────────────────────────────
// Socket client helpers (for CLI commands that proxy to daemon)
// ──────────────────────────────────────────────────────────────────────────────

/// Synchronous socket request using blocking std I/O on Unix sockets.
/// This avoids the runtime-in-runtime issue by using blocking operations.
fn socket_request(_socket_path: &PathBuf, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;

    let socket_path = default_socket_path();
    let mut stream = UnixStream::connect(&socket_path)
        .map_err(|e| anyhow::anyhow!("Failed to connect to daemon at {}: {}", socket_path.display(), e))?;

    let request = serde_json::json!({
        "method": method,
        "params": params,
    });
    let req_str = serde_json::to_string(&request)?;
    stream.write_all(req_str.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;

    // Read response line
    let mut reader = BufReader::new(&stream);
    let mut response = String::new();
    reader.read_line(&mut response)?;

    let resp: socket::JsonRpcResponse = serde_json::from_str(&response.trim())
        .map_err(|e| anyhow::anyhow!("invalid socket response: {} — raw: {}", e, response))?;

    if resp.ok {
        Ok(resp.result.unwrap_or(serde_json::Value::Null))
    } else {
        Err(anyhow::anyhow!(
            "socket error: {} (code {:?})",
            resp.error.unwrap_or_default(),
            resp.code
        ))
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// CLI commands
// ──────────────────────────────────────────────────────────────────────────────

async fn cmd_daemon_status(socket_path: &PathBuf) -> Result<()> {
    let params = serde_json::json!({});
    let result = socket_request(socket_path, "daemon.status", params)?;

    let chrome_running = result.get("chrome_running").and_then(|v| v.as_bool()).unwrap_or(false);
    let status = result.get("status").and_then(|v| v.as_str()).unwrap_or("unknown");

    println!("Daemon status: {}", status);
    println!("Chrome running: {}", chrome_running);

    if let Some(adapters) = result.get("adapters") {
        let total = adapters.get("total").and_then(|v| v.as_i64()).unwrap_or(0);
        let enabled = adapters.get("enabled").and_then(|v| v.as_i64()).unwrap_or(0);
        println!("Adapters: {} total, {} enabled", total, enabled);
    }

    if let Some(jobs) = result.get("jobs") {
        let pending = jobs.get("pending").and_then(|v| v.as_i64()).unwrap_or(0);
        let running = jobs.get("running").and_then(|v| v.as_i64()).unwrap_or(0);
        println!("Jobs: {} pending, {} running", pending, running);
    }

    Ok(())
}

async fn cmd_daemon_stop(socket_path: &PathBuf) -> Result<()> {
    // Connect and send a stop request
    let params = serde_json::json!({});
    if let Err(e) = socket_request(socket_path, "daemon.stop", params) {
        // Fallback: just remove the socket file
        if socket_path.exists() {
            std::fs::remove_file(socket_path)?;
            println!("Daemon stopped (socket removed)");
            return Ok(());
        }
        return Err(e);
    }
    println!("Daemon stopped");
    Ok(())
}

async fn cmd_adapter_list(
    socket_path: &PathBuf,
    include_disabled: bool,
    include_hidden: bool,
) -> Result<()> {
    let params = serde_json::json!({
        "include_disabled": include_disabled,
        "include_hidden": include_hidden,
    });
    let result = socket_request(socket_path, "adapter.list", params)?;

    let adapters = result.get("adapters").and_then(|v| v.as_array()).map_or(&[] as &[_], |v| v.as_slice());
    let count = result.get("count").and_then(|v| v.as_i64()).unwrap_or(0);

    if adapters.is_empty() {
        println!("No adapters found.");
        return Ok(());
    }

    println!("{:30} {:10} {:12} {}", "Name", "Enabled", "Browser", "Description");
    println!("{}", "-".repeat(80));
    for entry in adapters {
        let name = entry.get("full_name").and_then(|v| v.as_str()).unwrap_or("?");
        let enabled = entry.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
        let browser = entry.get("browser").and_then(|v| v.as_bool()).unwrap_or(false);
        let desc = entry.get("description").and_then(|v| v.as_str()).unwrap_or("");
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

async fn cmd_adapter_search(socket_path: &PathBuf, query: &str) -> Result<()> {
    let params = serde_json::json!({ "query": query });
    let result = socket_request(socket_path, "adapter.search", params)?;

    let adapters = result.get("adapters").and_then(|v| v.as_array()).map_or(&[] as &[_], |v| v.as_slice());
    let count = result.get("count").and_then(|v| v.as_i64()).unwrap_or(0);

    if adapters.is_empty() {
        println!("No adapters found matching '{}'.", query);
        return Ok(());
    }

    println!("{:30} {:12} {}", "Name", "Browser", "Description");
    println!("{}", "-".repeat(70));
    for entry in adapters {
        let name = entry.get("full_name").and_then(|v| v.as_str()).unwrap_or("?");
        let browser = entry.get("browser").and_then(|v| v.as_bool()).unwrap_or(false);
        let desc = entry.get("description").and_then(|v| v.as_str()).unwrap_or("");
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

async fn cmd_adapter_enable(socket_path: &PathBuf, name: &str) -> Result<()> {
    let params = serde_json::json!({ "name": name });
    let result = socket_request(socket_path, "adapter.enable", params)?;
    let enabled = result.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
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

async fn cmd_adapter_disable(socket_path: &PathBuf, name: &str) -> Result<()> {
    let params = serde_json::json!({ "name": name });
    let result = socket_request(socket_path, "adapter.disable", params)?;
    let enabled = result.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
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

async fn cmd_adapter_sync(socket_path: &PathBuf, folder: Option<PathBuf>) -> Result<()> {
    let params = serde_json::json!({
        "folder": folder.map(|p| p.display().to_string()),
    });
    let result = socket_request(socket_path, "adapter.sync", params)?;
    let synced = result.get("synced").and_then(|v| v.as_i64()).unwrap_or(0);
    let folder_str = result.get("folder").and_then(|v| v.as_str()).unwrap_or("?");
    println!("Synced {} adapters from '{}'", synced, folder_str);
    Ok(())
}

// ──────────────────────────────────────────────────────────────────────────────
// Main
// ──────────────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let _subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .with_target(false)
        .compact()
        .init();

    let args = Cli::parse();

    match args.command {
        Command::Start { poll_interval, db, socket, no_load_adapters: _ } => {
            let sp = socket.unwrap_or_else(default_socket_path);
            run_daemon(poll_interval, db, sp, false).await?;
        }

        Command::Stop { socket } => {
            let sp = socket.unwrap_or_else(default_socket_path);
            cmd_daemon_stop(&sp).await?;
        }

        Command::Status { socket } => {
            let sp = socket.unwrap_or_else(default_socket_path);
            cmd_daemon_status(&sp).await?;
        }

        Command::Restart { poll_interval, db, socket } => {
            let sp = socket.unwrap_or_else(default_socket_path);
            // Try to stop first
            let _ = cmd_daemon_stop(&sp).await;
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            run_daemon(poll_interval, db, sp, false).await?;
        }

        Command::Job { sub } => {
            match sub {
                JobSubcommand::Add { adapter, run_at, delay, interval, args: args_json } => {
                    let store = open_store(None)?;

                    let run_at_dt = match (run_at.as_deref(), delay) {
                        (Some(s), _) => parse_datetime(s)?,
                        (None, Some(d)) => Utc::now() + Duration::seconds(d),
                        (None, None) => Utc::now(),
                    };

                    let args_val: Option<Value> = args_json
                        .as_ref()
                        .and_then(|s| serde_json::from_str(s).ok());

                    let job = store.add(&adapter, args_val, run_at_dt, interval)?;
                    println!("Job created: {}", job.id);
                    println!("   Adapter: {}  |  Run at: {}  |  Status: {}",
                        job.adapter, job.run_at, job.status);
                    if let Some(i) = job.interval_seconds {
                        println!("   Interval: {}s", i);
                    }
                }
                JobSubcommand::List { status, limit } => {
                    let store = open_store(None)?;
                    let status_filter = status.as_ref().map(|s| JobStatus::from(s.as_str()));
                    let jobs = store.list(status_filter, limit)?;

                    if jobs.is_empty() {
                        println!("No jobs found.");
                        return Ok(());
                    }

                    println!("{:40} {:20} {:12} {:20}", "ID", "Adapter", "Status", "Run At");
                    println!("{}", "-".repeat(95));
                    for job in &jobs {
                        println!("{:40} {:20} {:12} {}",
                            &job.id[..8.min(job.id.len())],
                            &job.adapter,
                            job.status,
                            job.run_at.format("%Y-%m-%d %H:%M")
                        );
                    }
                    println!("\n{} jobs total", jobs.len());
                }
                JobSubcommand::Show { id } => {
                    let store = open_store(None)?;
                    match store.get(&id)? {
                        Some(job) => {
                            println!("ID:       {}", job.id);
                            println!("Adapter:  {}", job.adapter);
                            println!("Status:   {}", job.status);
                            println!("Run at:   {}", job.run_at);
                            if let Some(i) = job.interval_seconds {
                                println!("Interval: {}s", i);
                            }
                            println!("Retries:  {}/{}", job.retry_count, job.max_retries);
                            if let Some(start) = job.start_at {
                                println!("Start:    {}", start);
                            }
                            if let Some(end) = job.end_at {
                                println!("End:      {}", end);
                            }
                            if let Some(args) = &job.args {
                                println!("Args:     {}", serde_json::to_string_pretty(args).unwrap_or_default());
                            }
                            if let Some(result) = &job.result {
                                println!("Result:   {}", result.chars().take(200).collect::<String>());
                            }
                            if let Some(error) = &job.error {
                                println!("Error:    {}", error);
                            }
                        }
                        None => {
                            println!("Job not found: {}", id);
                        }
                    }
                }
                JobSubcommand::Cancel { id } => {
                    let store = open_store(None)?;
                    store.cancel(&id)?;
                    println!("Cancelled: {}", id);
                }
                JobSubcommand::Delete { id } => {
                    let store = open_store(None)?;
                    store.delete(&id)?;
                    println!("Deleted: {}", id);
                }
                JobSubcommand::Run => {
                    // Inline scheduler run (no daemon needed)
                    let store = Arc::new(open_store(None)?);
                    let am = Arc::new(AdapterManager::new().await?);
                    let scheduler = Scheduler::new(Arc::clone(&store), Arc::clone(&am), 1);
                    scheduler.poll_and_run().await?;
                }
            }
        }

        Command::Adapter { sub } => {
            // All adapter subcommands go through the socket
            let sp = default_socket_path();
            match sub {
                AdapterSubcommand::List { include_disabled, include_hidden } => {
                    cmd_adapter_list(&sp, include_disabled, include_hidden).await?;
                }
                AdapterSubcommand::Search { query } => {
                    cmd_adapter_search(&sp, &query).await?;
                }
                AdapterSubcommand::Enable { name } => {
                    cmd_adapter_enable(&sp, &name).await?;
                }
                AdapterSubcommand::Disable { name } => {
                    cmd_adapter_disable(&sp, &name).await?;
                }
                AdapterSubcommand::Sync { folder } => {
                    cmd_adapter_sync(&sp, folder).await?;
                }
            }
        }

        Command::Socket { args: raw_args } => {
            // Raw socket command: pass JSON args directly
            if raw_args.is_empty() {
                anyhow::bail!("Usage: socket <json...>");
            }
            let input = raw_args.join(" ");
            let sp = default_socket_path();

            // Handle ping specially (no socket needed for this)
            if input.contains("daemon.ping") {
                println!("{}", socket_request(&sp, "daemon.ping", serde_json::json!({}))?);
                return Ok(());
            }

            let resp: socket::JsonRpcResponse = serde_json::from_str(&input)
                .or_else(|_| serde_json::from_str::<serde_json::Value>(&input)
                    .map(|v| socket::JsonRpcResponse {
                        ok: true,
                        result: Some(v),
                        error: None,
                        code: None,
                        id: None,
                    }))
                .map_err(|e| anyhow::anyhow!("invalid input: {}", e))?;

            if !resp.ok {
                eprintln!("Error: {} (code {:?})", resp.error.unwrap_or_default(), resp.code);
            } else if let Some(result) = resp.result {
                println!("{}", serde_json::to_string_pretty(&result).unwrap_or_default());
            }
        }
    }

    Ok(())
}
