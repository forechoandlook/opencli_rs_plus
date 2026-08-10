//! Comprehensive socket API server for daemon communication.
//! Uses a JSON-RPC-like protocol over TCP sockets.

use crate::adapter_manager::{is_chrome_running, AdapterManager};
use crate::plugin::PluginManager;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tracing::{error, info};

/// Shared state accessible by all socket handlers.
pub struct SocketState {
    pub adapter_manager: Arc<AdapterManager>,
    pub plugin_manager: Arc<PluginManager>,
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

/// Handle a single TCP connection: line-delimited JSON-RPC request/response.
async fn handle_connection(stream: tokio::net::TcpStream, state: &Arc<SocketState>) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    while let Some(line) = lines.next_line().await? {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let response = process_request(line, state).await;
        let resp_json = serde_json::to_string(&response)?;
        writer.write_all(resp_json.as_bytes()).await?;
        writer.write_all(b"\n").await?;
    }

    Ok(())
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
        "adapter.list" => handle_adapter_list(params, state).await,
        "adapter.search" => handle_adapter_search(params, state).await,
        "adapter.enable" => handle_adapter_enable(params, state).await,
        "adapter.disable" => handle_adapter_disable(params, state).await,
        "adapter.reload" => handle_adapter_reload(state).await,

        // ── Plugin ────────────────────────────────────────────────────────────
        "plugin.install" => handle_plugin_install(params, state).await,
        "plugin.uninstall" => handle_plugin_uninstall(params, state).await,
        "plugin.list" => handle_plugin_list(state).await,
        "plugin.update" => handle_plugin_update(params, state).await,

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
    // Exit after a short delay so the "stopping" response reaches the client first.
    tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let _ = std::fs::remove_file(crate::default_pid_path());
        std::process::exit(0);
    });
    Ok(serde_json::json!({ "stopping": true }))
}

// ──────────────────────────────────────────────────────────────────────────────
// Adapter handlers
// ──────────────────────────────────────────────────────────────────────────────

async fn handle_adapter_list(params: &Value, state: &Arc<SocketState>) -> Result<Value> {
    let include_disabled = params
        .get("include_disabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let adapters = state.adapter_manager.list_adapters().await;
    let filtered: Vec<_> = adapters
        .into_iter()
        .filter(|a| include_disabled || a.enabled)
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

    let results = state.adapter_manager.search(query).await;
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
    Ok(serde_json::json!({ "loaded": count }))
}

// ──────────────────────────────────────────────────────────────────────────────
// Plugin handlers
// ──────────────────────────────────────────────────────────────────────────────

async fn handle_plugin_install(params: &Value, state: &Arc<SocketState>) -> Result<Value> {
    let source = params
        .get("path")
        .or_else(|| params.get("source"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing 'path' parameter"))?;

    let info = state.plugin_manager.install(source).await?;
    // Reload so the new plugin's adapters are immediately available
    state.adapter_manager.reload().await?;
    Ok(serde_json::json!({ "plugin": info }))
}

async fn handle_plugin_uninstall(params: &Value, state: &Arc<SocketState>) -> Result<Value> {
    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing 'name' parameter"))?;

    state.plugin_manager.uninstall(name).await?;
    state.adapter_manager.reload().await?;
    Ok(serde_json::json!({ "uninstalled": name }))
}

async fn handle_plugin_list(state: &Arc<SocketState>) -> Result<Value> {
    let plugins = state.plugin_manager.list()?;
    let count = plugins.len();
    Ok(serde_json::json!({ "plugins": plugins, "count": count }))
}

async fn handle_plugin_update(params: &Value, state: &Arc<SocketState>) -> Result<Value> {
    match params.get("name").and_then(|v| v.as_str()) {
        Some(name) => {
            state.plugin_manager.update(name).await?;
            state.adapter_manager.reload().await?;
            Ok(serde_json::json!({ "updated": [name] }))
        }
        None => {
            // Update all installed plugins
            let results = state.plugin_manager.update_all().await;
            let mut updated = vec![];
            let mut errors = vec![];
            for (name, result) in results {
                match result {
                    Ok(_) => updated.push(name),
                    Err(e) => {
                        errors.push(serde_json::json!({ "plugin": name, "error": e.to_string() }))
                    }
                }
            }
            if !updated.is_empty() {
                state.adapter_manager.reload().await?;
            }
            Ok(serde_json::json!({ "updated": updated, "errors": errors }))
        }
    }
}
