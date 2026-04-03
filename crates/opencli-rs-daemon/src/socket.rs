//! Comprehensive socket API server for daemon communication.
//! Uses a JSON-RPC-like protocol over TCP sockets.

use crate::adapter_manager::{is_chrome_running, AdapterManager};
use crate::scheduler::Scheduler;
use crate::store::{Job, JobStatus, JobStore};
use anyhow::Result;
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::net::tcp::OwnedWriteHalf;
use tracing::{error, info};

/// Shared state accessible by all socket handlers.
pub struct SocketState {
    pub adapter_manager: Arc<AdapterManager>,
    pub scheduler: Arc<Scheduler>,
    pub job_store: Arc<JobStore>,
}

/// JSON-RPC-like request
#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    pub method: String,
    #[serde(default)]
    pub params: Value,
    #[serde(default)]
    pub id: Option<Value>,
}

/// JSON-RPC-like response
#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
}

impl JsonRpcResponse {
    #[allow(dead_code)]
    fn success(result: Value) -> Self {
        Self {
            ok: true,
            result: Some(result),
            error: None,
            code: None,
            id: None,
        }
    }

    fn success_with_id(result: Value, id: Option<Value>) -> Self {
        Self {
            ok: true,
            result: Some(result),
            error: None,
            code: None,
            id,
        }
    }

    fn error(msg: &str, code: i32) -> Self {
        Self {
            ok: false,
            result: None,
            error: Some(msg.to_string()),
            code: Some(code),
            id: None,
        }
    }

    fn error_with_id(msg: &str, code: i32, id: Option<Value>) -> Self {
        Self {
            ok: false,
            result: None,
            error: Some(msg.to_string()),
            code: Some(code),
            id,
        }
    }
}

/// Stream event for `exec` command
#[derive(Debug, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum StreamEvent {
    #[serde(rename = "stdout")]
    Stdout(String),
    #[serde(rename = "stderr")]
    Stderr(String),
    #[serde(rename = "done")]
    Done { exit_code: i32 },
}

/// Start the TCP socket server. Each connection is handled concurrently.
pub async fn serve(addr: &str, state: Arc<SocketState>) -> Result<()> {
    let listener = TcpListener::bind(addr).await?;
    info!(addr = %addr, "Socket server listening");

    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                info!(peer = %peer, "New connection");
                let state = Arc::clone(&state);
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream, &state).await {
                        error!(error = %e, "Connection handler error");
                    }
                });
            }
            Err(e) => {
                error!(error = %e, "Socket accept error");
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            }
        }
    }
}

/// Handle a single TCP connection.
/// Reads line-delimited JSON requests, writes line-delimited JSON responses.
/// For `exec`, streams JSON lines until done.
async fn handle_connection(
    stream: tokio::net::TcpStream,
    state: &Arc<SocketState>,
) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    while let Some(line) = lines.next_line().await? {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Check if this is an exec request before processing
        let is_exec = line.contains(r#""method":"exec""#);

        if is_exec {
            // Parse and handle exec with streaming
            let req: JsonRpcRequest = match serde_json::from_str(&line) {
                Ok(r) => r,
                Err(e) => {
                    let resp = JsonRpcResponse::error(&format!("invalid JSON: {}", e), -32700);
                    writer.write_all(serde_json::to_string(&resp)?.as_bytes()).await?;
                    writer.write_all(b"\n").await?;
                    continue;
                }
            };

            let id = req.id.clone();
            let exec_result = handle_exec_streaming(&req.params, state, &mut writer).await;

            match exec_result {
                Ok(exit_code) => {
                    let done = StreamEvent::Done { exit_code };
                    writer.write_all(serde_json::to_string(&done)?.as_bytes()).await?;
                    writer.write_all(b"\n").await?;
                }
                Err(e) => {
                    let resp = JsonRpcResponse::error_with_id(&e.to_string(), -32603, id);
                    writer.write_all(serde_json::to_string(&resp)?.as_bytes()).await?;
                    writer.write_all(b"\n").await?;
                }
            }
        } else {
            let response = process_request(&line, state).await;
            let resp_json = serde_json::to_string(&response)?;
            writer.write_all(resp_json.as_bytes()).await?;
            writer.write_all(b"\n").await?;
        }
    }

    Ok(())
}

/// Handle exec command with streaming output to the writer.
async fn handle_exec_streaming(
    params: &Value,
    state: &Arc<SocketState>,
    writer: &mut OwnedWriteHalf,
) -> Result<i32> {
    let adapter = params
        .get("adapter")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing 'adapter' parameter"))?;

    let args = params.get("args").cloned();

    // Parse "site command"
    let parts: Vec<&str> = adapter.split_whitespace().collect();
    if parts.len() != 2 {
        return Err(anyhow::anyhow!("Invalid adapter format: '{}'", adapter));
    }
    let (site, cmd_name) = (parts[0], parts[1]);

    // Get command from adapter manager
    let cmd = match state.adapter_manager.get_command(site, cmd_name).await {
        Some(c) => c,
        None => {
            return Err(anyhow::anyhow!("Unknown or disabled adapter: {} {}", site, cmd_name));
        }
    };

    let kwargs: std::collections::HashMap<String, Value> = match &args {
        Some(serde_json::Value::Object(map)) => {
            map.iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        }
        Some(_) => {
            return Err(anyhow::anyhow!("args must be a JSON object"));
        }
        None => std::collections::HashMap::new(),
    };

    // Execute the command
    match opencli_rs_cli::execute_command(&cmd, kwargs).await {
        Ok(result) => {
            // Stream result as stdout
            let stdout = serde_json::to_string(&result)?;
            let event = StreamEvent::Stdout(stdout);
            writer.write_all(serde_json::to_string(&event)?.as_bytes()).await?;
            writer.write_all(b"\n").await?;
            Ok(0)
        }
        Err(e) => {
            // Stream error as stderr
            let stderr = e.to_string();
            let event = StreamEvent::Stderr(stderr);
            writer.write_all(serde_json::to_string(&event)?.as_bytes()).await?;
            writer.write_all(b"\n").await?;
            Ok(1)
        }
    }
}

/// Process a single JSON-RPC request and return the response.
async fn process_request(line: &str, state: &Arc<SocketState>) -> JsonRpcResponse {
    // Parse request
    let req: JsonRpcRequest = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(e) => return JsonRpcResponse::error(&format!("invalid JSON: {}", e), -32700),
    };

    let method = &req.method;
    let params = &req.params;
    let id = req.id.clone();

    let result = match method.as_str() {
        // ── Daemon ──────────────────────────────────────────────────────────────
        "daemon.status" => handle_daemon_status(state).await,
        "daemon.ping" => handle_pong(),
        "daemon.stop" => handle_daemon_stop(),

        // ── Adapter ────────────────────────────────────────────────────────────
        "adapter.sync" => handle_adapter_sync(params, state).await,
        "adapter.list" => handle_adapter_list(params, state).await,
        "adapter.search" => handle_adapter_search(params, state).await,
        "adapter.enable" => handle_adapter_enable(params, state).await,
        "adapter.disable" => handle_adapter_disable(params, state).await,
        "adapter.reload" => handle_adapter_reload(state).await,

        // ── Job ───────────────────────────────────────────────────────────────
        "job.add" => handle_job_add(params, state).await,
        "job.list" => handle_job_list(params, state).await,
        "job.show" => handle_job_show(params, state).await,
        "job.cancel" => handle_job_cancel(params, state).await,
        "job.delete" => handle_job_delete(params, state).await,
        "job.run" => handle_job_run(state).await,

        // Note: "exec" is handled separately in handle_connection for streaming.
        _ => Err(anyhow::anyhow!("unknown method: {}", method)),
    };

    match result {
        Ok(v) => JsonRpcResponse::success_with_id(v, id),
        Err(e) => JsonRpcResponse::error_with_id(&e.to_string(), -32603, id),
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Daemon handlers
// ──────────────────────────────────────────────────────────────────────────────

async fn handle_daemon_status(state: &Arc<SocketState>) -> Result<Value> {
    let chrome_running = is_chrome_running();

    let job_store = &state.job_store;
    let pending: usize = job_store.list(Some(JobStatus::Pending), 1000).map(|j: Vec<Job>| j.len()).unwrap_or(0);
    let running: usize = job_store.list(Some(JobStatus::Running), 1000).map(|j: Vec<Job>| j.len()).unwrap_or(0);

    let am = state.adapter_manager.list_adapters().await;
    let total = am.len();
    let enabled = am.iter().filter(|a| a.enabled).count();

    Ok(serde_json::json!({
        "status": "running",
        "chrome_running": chrome_running,
        "adapters": {
            "total": total,
            "enabled": enabled,
            "disabled": total - enabled,
        },
        "jobs": {
            "pending": pending,
            "running": running,
        },
        "uptime_seconds": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    }))
}

fn handle_pong() -> Result<Value> {
    Ok(serde_json::json!({ "pong": true }))
}

fn handle_daemon_stop() -> Result<Value> {
    // Signal the daemon to shut down by sending tokio signal
    // In practice this sets a shutdown flag; the main loop will detect and exit.
    // For now, we just acknowledge and let the caller handle the exit.
    Ok(serde_json::json!({ "stopping": true }))
}

// ──────────────────────────────────────────────────────────────────────────────
// Adapter handlers
// ──────────────────────────────────────────────────────────────────────────────

async fn handle_adapter_sync(params: &Value, state: &Arc<SocketState>) -> Result<Value> {
    let folder = params
        .get("folder")
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .map(|h| h.join(".opencli-rs").join("adapters"))
                .unwrap_or_default()
        });

    let count = state.adapter_manager.sync_from(&folder).await?;
    Ok(serde_json::json!({
        "synced": count,
        "folder": folder.display().to_string(),
    }))
}

async fn handle_adapter_list(params: &Value, state: &Arc<SocketState>) -> Result<Value> {
    let include_disabled = params.get("include_disabled").and_then(|v| v.as_bool()).unwrap_or(false);
    let include_hidden = params.get("include_hidden").and_then(|v| v.as_bool()).unwrap_or(false);

    let adapters = state.adapter_manager.list_adapters().await;
    let filtered: Vec<_> = adapters
        .into_iter()
        .filter(|a| {
            (include_disabled || a.enabled) && (include_hidden || !a.hidden)
        })
        .collect();

    Ok(serde_json::json!({
        "adapters": filtered,
        "count": filtered.len(),
    }))
}

async fn handle_adapter_search(params: &Value, state: &Arc<SocketState>) -> Result<Value> {
    let query = params
        .get("query")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let include_hidden = params.get("include_hidden").and_then(|v| v.as_bool()).unwrap_or(false);

    let results = state.adapter_manager.search(query, include_hidden).await;
    Ok(serde_json::json!({
        "query": query,
        "adapters": results,
        "count": results.len(),
    }))
}

async fn handle_adapter_enable(params: &Value, state: &Arc<SocketState>) -> Result<Value> {
    let full_name = params
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing 'name' parameter"))?;

    let now_enabled = state.adapter_manager.enable(full_name).await?;
    Ok(serde_json::json!({
        "name": full_name,
        "enabled": now_enabled,
    }))
}

async fn handle_adapter_disable(params: &Value, state: &Arc<SocketState>) -> Result<Value> {
    let full_name = params
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing 'name' parameter"))?;

    let now_disabled = state.adapter_manager.disable(full_name).await?;
    Ok(serde_json::json!({
        "name": full_name,
        "enabled": !now_disabled,
    }))
}

async fn handle_adapter_reload(state: &Arc<SocketState>) -> Result<Value> {
    let count = state.adapter_manager.reload().await?;
    Ok(serde_json::json!({
        "loaded": count,
    }))
}

// ──────────────────────────────────────────────────────────────────────────────
// Job handlers
// ──────────────────────────────────────────────────────────────────────────────

async fn handle_job_add(params: &Value, state: &Arc<SocketState>) -> Result<Value> {
    let adapter = params
        .get("adapter")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing 'adapter' parameter"))?;

    let args = params.get("args").cloned();
    let delay_seconds = params.get("delay_seconds").and_then(|v| v.as_i64());
    let interval_seconds = params.get("interval_seconds").and_then(|v| v.as_i64());

    let run_at = match delay_seconds {
        Some(d) => Utc::now() + Duration::seconds(d),
        None => Utc::now(),
    };

    let job = state.job_store.add(adapter, args, run_at, interval_seconds)?;
    Ok(serde_json::json!({
        "job": job,
    }))
}

async fn handle_job_list(params: &Value, state: &Arc<SocketState>) -> Result<Value> {
    let status_filter = params
        .get("status")
        .and_then(|v| v.as_str())
        .map(JobStatus::from);
    let limit = params
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(50) as usize;

    let jobs = state.job_store.list(status_filter, limit)?;
    Ok(serde_json::json!({
        "jobs": jobs,
        "count": jobs.len(),
    }))
}

async fn handle_job_show(params: &Value, state: &Arc<SocketState>) -> Result<Value> {
    let id = params
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing 'id' parameter"))?;

    match state.job_store.get(id)? {
        Some(job) => Ok(serde_json::json!({ "job": job })),
        None => Err(anyhow::anyhow!("job not found: {}", id)),
    }
}

async fn handle_job_cancel(params: &Value, state: &Arc<SocketState>) -> Result<Value> {
    let id = params
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing 'id' parameter"))?;

    state.job_store.cancel(id)?;
    Ok(serde_json::json!({ "id": id, "cancelled": true }))
}

async fn handle_job_delete(params: &Value, state: &Arc<SocketState>) -> Result<Value> {
    let id = params
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing 'id' parameter"))?;

    state.job_store.delete(id)?;
    Ok(serde_json::json!({ "id": id, "deleted": true }))
}

async fn handle_job_run(state: &Arc<SocketState>) -> Result<Value> {
    state.scheduler.poll_and_run().await?;
    Ok(serde_json::json!({ "ran": true }))
}
