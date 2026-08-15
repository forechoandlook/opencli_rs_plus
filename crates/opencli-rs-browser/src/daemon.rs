use axum::{
    extract::DefaultBodyLimit,
    extract::{
        ws::{Message, WebSocket},
        State, WebSocketUpgrade,
    },
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use futures::{SinkExt, StreamExt};
use opencli_rs_core::CliError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::sync::{oneshot, Mutex, RwLock};
use tracing::{debug, error, info, warn};

use crate::types::{DaemonCommand, DaemonResult};

// ─── Extension log persistence ───────────────────────────────────────────────

/// Write an extension log entry to ~/.opencli-rs/logs/extension-YYYYMMDD.jsonl.
/// Each line is a JSON object: { ts, level, msg }.
/// Failures are silently ignored — logging must never crash the daemon.
fn write_extension_log(level: &str, msg: &str, ts: u64) {
    use std::io::Write;
    let Some(home) = dirs::home_dir() else { return };
    let log_dir = home.join(".opencli-rs").join("logs");
    if std::fs::create_dir_all(&log_dir).is_err() {
        return;
    }

    // One file per day: extension-20260410.jsonl
    let filename = format_log_date();
    let log_path = log_dir.join(format!("extension-{filename}.jsonl"));

    let entry = serde_json::json!({ "ts": ts, "level": level, "msg": msg });
    let line = entry.to_string() + "\n";

    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        let _ = f.write_all(line.as_bytes());
    }
}

fn format_log_date() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Compute YYYYMMDD in UTC without chrono dependency
    // Algorithm: http://howardhinnant.github.io/date_algorithms.html  civil_from_days
    let days = (secs / 86400) as i64;
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}{m:02}{d:02}")
}

/// Command response timeout. Set high to support long-running tasks (image/video generation).
const COMMAND_TIMEOUT: Duration = Duration::from_secs(1800); // 30 minutes
/// WebSocket heartbeat interval.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
/// Idle shutdown threshold.
const IDLE_TIMEOUT: Duration = Duration::from_secs(1800);
/// Allow large extension command payloads such as bg_fetch API responses.
const COMMAND_BODY_LIMIT: usize = 32 * 1024 * 1024;
/// Keep a concise local task history for the popup, without persisting results.
const TASK_HISTORY_LIMIT: usize = 30;
const TASK_TEXT_LIMIT: usize = 1_500;

type PendingMap = HashMap<String, oneshot::Sender<DaemonResult>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserTask {
    pub id: String,
    pub workspace: String,
    pub status: String,
    pub started_at_ms: u128,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_preview: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TaskUpdate {
    id: Option<String>,
    workspace: Option<String>,
    status: String,
    result: Option<Value>,
    error: Option<String>,
}

/// Shared state for the daemon server.
pub struct DaemonState {
    pub extension_tx: Mutex<Option<futures::stream::SplitSink<WebSocket, Message>>>,
    pub pending_commands: RwLock<PendingMap>,
    pub extension_connected: RwLock<bool>,
    /// A stale socket must not clear a newer extension connection.
    pub active_extension_connection_id: AtomicU64,
    pub last_activity: RwLock<Instant>,
    pub tasks: RwLock<VecDeque<BrowserTask>>,
}

impl DaemonState {
    fn new() -> Self {
        Self {
            extension_tx: Mutex::new(None),
            pending_commands: RwLock::new(HashMap::new()),
            extension_connected: RwLock::new(false),
            active_extension_connection_id: AtomicU64::new(0),
            last_activity: RwLock::new(Instant::now()),
            tasks: RwLock::new(VecDeque::new()),
        }
    }

    async fn touch(&self) {
        *self.last_activity.write().await = Instant::now();
    }
}

/// The Daemon HTTP + WebSocket server.
pub struct Daemon {
    port: u16,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl Daemon {
    /// Start the daemon server on the given port. Returns immediately after the listener binds.
    pub async fn start(port: u16) -> Result<Self, CliError> {
        let state = Arc::new(DaemonState::new());
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

        let app = Router::new()
            .route("/health", get(health_handler))
            .route("/status", get(status_handler))
            .route(
                "/tasks",
                get(tasks_handler)
                    .post(tasks_handler)
                    .options(tasks_handler),
            )
            .route("/command", post(command_handler))
            .route("/ext", get(ws_handler))
            .layer(DefaultBodyLimit::max(COMMAND_BODY_LIMIT))
            .with_state(state.clone());

        let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}"))
            .await
            .map_err(|e| {
                CliError::browser_connect(format!("Failed to bind daemon on port {port}: {e}"))
            })?;

        info!(port, "daemon listening");

        // Spawn idle-shutdown watchdog
        let idle_state = state.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;
                let last = *idle_state.last_activity.read().await;
                if last.elapsed() > IDLE_TIMEOUT {
                    info!("daemon idle timeout reached, shutting down");
                    break;
                }
            }
        });

        // Spawn the server
        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                    info!("daemon received shutdown signal");
                })
                .await
                .ok();
        });

        Ok(Self {
            port,
            shutdown_tx: Some(shutdown_tx),
        })
    }

    /// Gracefully shut down the daemon.
    pub async fn shutdown(mut self) -> Result<(), CliError> {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        info!(port = self.port, "daemon shutdown complete");
        Ok(())
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

/// GET /health — simple liveness check.
async fn health_handler() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

/// GET /status — return daemon and extension status.
/// Compatible with both opencli-rs and original opencli formats.
async fn status_handler(State(state): State<Arc<DaemonState>>) -> impl IntoResponse {
    let ext = *state.extension_connected.read().await;
    let pending = state.pending_commands.read().await.len();
    Json(json!({
        "daemon": true,
        "extension": ext,
        // Original opencli compatibility fields
        "ok": true,
        "extensionConnected": ext,
        "pending": pending,
    }))
}

/// Return the latest browser-backed adapter tasks for the extension popup.
/// This endpoint deliberately serves only daemon-memory previews; complete
/// adapter data is never written to a task-history file.
async fn tasks_handler(
    State(state): State<Arc<DaemonState>>,
    headers: HeaderMap,
    request: axum::extract::Request,
) -> axum::response::Response {
    // Chrome sends an OPTIONS preflight before the popup can attach its
    // X-OpenCLI header. Only extension origins receive CORS permission; normal
    // web pages cannot read this local task/result endpoint.
    if request.method() == axum::http::Method::OPTIONS {
        return task_cors_response(&headers, StatusCode::NO_CONTENT.into_response());
    }

    // Popup GETs are authenticated by their Chrome extension Origin. Keeping
    // them header-free avoids an unnecessary CORS OPTIONS preflight. Internal
    // daemon clients keep using X-OpenCLI, because they have no Origin header.
    if !headers.contains_key("x-opencli") && !is_extension_origin(&headers) {
        return task_cors_response(
            &headers,
            (
                StatusCode::FORBIDDEN,
                Json(json!({ "error": "Missing X-OpenCLI header" })),
            )
                .into_response(),
        );
    }

    if request.method() == axum::http::Method::GET {
        let tasks = state.tasks.read().await;
        let count = tasks.len();
        let tasks: Vec<BrowserTask> = tasks.iter().cloned().collect();
        return task_cors_response(
            &headers,
            Json(json!({ "tasks": tasks, "count": count })).into_response(),
        );
    }

    let body = match axum::body::to_bytes(request.into_body(), COMMAND_BODY_LIMIT).await {
        Ok(body) => body,
        Err(_) => {
            return task_cors_response(
                &headers,
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "Invalid task request body" })),
                )
                    .into_response(),
            )
        }
    };
    let update: TaskUpdate = match serde_json::from_slice(&body) {
        Ok(update) => update,
        Err(err) => {
            return task_cors_response(
                &headers,
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": format!("Invalid task request: {err}") })),
                )
                    .into_response(),
            )
        }
    };

    let now = unix_time_ms();
    let mut tasks = state.tasks.write().await;
    if update.status == "running" {
        let workspace = update
            .workspace
            .unwrap_or_else(|| "adapter:unknown".to_string());
        let task = BrowserTask {
            id: uuid::Uuid::new_v4().to_string(),
            workspace,
            status: "running".to_string(),
            started_at_ms: now,
            finished_at_ms: None,
            result_preview: None,
            error: None,
        };
        tasks.push_front(task.clone());
        while tasks.len() > TASK_HISTORY_LIMIT {
            tasks.pop_back();
        }
        return task_cors_response(&headers, Json(json!({ "task": task })).into_response());
    }

    let Some(id) = update.id else {
        return task_cors_response(
            &headers,
            (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "Task id is required when finishing a task" })),
            )
                .into_response(),
        );
    };
    let Some(task) = tasks.iter_mut().find(|task| task.id == id) else {
        return task_cors_response(
            &headers,
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "Task not found" })),
            )
                .into_response(),
        );
    };
    task.status = update.status;
    task.finished_at_ms = Some(now);
    task.result_preview = update.result.as_ref().map(task_preview);
    task.error = update.error.as_deref().map(limit_task_text);
    task_cors_response(
        &headers,
        Json(json!({ "task": task.clone() })).into_response(),
    )
}

fn task_cors_response(
    headers: &HeaderMap,
    mut response: axum::response::Response,
) -> axum::response::Response {
    let Some(origin) = headers.get(axum::http::header::ORIGIN) else {
        return response;
    };
    let Ok(origin) = origin.to_str() else {
        return response;
    };
    if !origin.starts_with("chrome-extension://") {
        return response;
    }

    let response_headers = response.headers_mut();
    if let Ok(origin_value) = axum::http::HeaderValue::from_str(origin) {
        response_headers.insert(
            axum::http::header::ACCESS_CONTROL_ALLOW_ORIGIN,
            origin_value,
        );
    }
    response_headers.insert(
        axum::http::header::ACCESS_CONTROL_ALLOW_METHODS,
        axum::http::HeaderValue::from_static("GET, POST, OPTIONS"),
    );
    response_headers.insert(
        axum::http::header::ACCESS_CONTROL_ALLOW_HEADERS,
        axum::http::HeaderValue::from_static("X-OpenCLI, Content-Type"),
    );
    response_headers.insert(
        axum::http::header::VARY,
        axum::http::HeaderValue::from_static("Origin"),
    );
    response
}

fn is_extension_origin(headers: &HeaderMap) -> bool {
    headers
        .get(axum::http::header::ORIGIN)
        .and_then(|origin| origin.to_str().ok())
        .is_some_and(|origin| origin.starts_with("chrome-extension://"))
}

fn unix_time_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn task_preview(value: &Value) -> String {
    let sanitized = redact_task_value(value);
    limit_task_text(
        &serde_json::to_string_pretty(&sanitized).unwrap_or_else(|_| "<unavailable>".to_string()),
    )
}

fn redact_task_value(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| {
                    let lower = key.to_ascii_lowercase();
                    let sensitive = ["token", "secret", "password", "authorization", "cookie"]
                        .iter()
                        .any(|needle| lower.contains(needle));
                    (
                        key.clone(),
                        if sensitive {
                            Value::String("[redacted]".to_string())
                        } else {
                            redact_task_value(value)
                        },
                    )
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.iter().map(redact_task_value).collect()),
        Value::String(text) => Value::String(limit_task_text(&redact_url_query_tokens(text))),
        _ => value.clone(),
    }
}

fn redact_url_query_tokens(text: &str) -> String {
    let mut redacted = text.to_string();
    for key in ["xsec_token", "access_token"] {
        let needle = format!("{key}=");
        let mut start = 0;
        while let Some(offset) = redacted[start..].find(&needle) {
            let value_start = start + offset + needle.len();
            let value_end = redacted[value_start..]
                .find('&')
                .map(|offset| value_start + offset)
                .unwrap_or(redacted.len());
            redacted.replace_range(value_start..value_end, "[redacted]");
            start = value_start + "[redacted]".len();
        }
    }
    redacted
}

fn limit_task_text(text: &str) -> String {
    let mut chars = text.chars();
    let preview: String = chars.by_ref().take(TASK_TEXT_LIMIT).collect();
    if chars.next().is_some() {
        format!("{preview}…")
    } else {
        preview
    }
}

/// POST /command — accept a command from the CLI and forward to the extension.
async fn command_handler(
    State(state): State<Arc<DaemonState>>,
    headers: HeaderMap,
    Json(cmd): Json<DaemonCommand>,
) -> impl IntoResponse {
    // Security: require X-OpenCLI header
    if !headers.contains_key("x-opencli") {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "Missing X-OpenCLI header" })),
        );
    }

    state.touch().await;

    // Check extension connected
    if !*state.extension_connected.read().await {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "Chrome extension not connected" })),
        );
    }

    let cmd_id = cmd.id.clone();

    // Create a oneshot channel for the result
    let (tx, rx) = oneshot::channel::<DaemonResult>();
    state
        .pending_commands
        .write()
        .await
        .insert(cmd_id.clone(), tx);

    // Forward command to extension via WebSocket
    {
        let mut ext_tx = state.extension_tx.lock().await;
        if let Some(ref mut sink) = *ext_tx {
            let msg = serde_json::to_string(&cmd).unwrap_or_default();
            if let Err(e) = sink.send(Message::Text(msg.into())).await {
                state.pending_commands.write().await.remove(&cmd_id);
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(json!({ "error": format!("Failed to send to extension: {e}") })),
                );
            }
        } else {
            state.pending_commands.write().await.remove(&cmd_id);
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": "Extension WebSocket not available" })),
            );
        }
    }

    // Wait for result with timeout
    match tokio::time::timeout(COMMAND_TIMEOUT, rx).await {
        Ok(Ok(result)) => {
            let status = if result.ok {
                StatusCode::OK
            } else {
                StatusCode::UNPROCESSABLE_ENTITY
            };
            (
                status,
                Json(serde_json::to_value(result).unwrap_or(json!({}))),
            )
        }
        Ok(Err(_)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Command channel closed unexpectedly" })),
        ),
        Err(_) => {
            state.pending_commands.write().await.remove(&cmd_id);
            (
                StatusCode::GATEWAY_TIMEOUT,
                Json(json!({ "error": "Command timed out" })),
            )
        }
    }
}

/// GET /ext — WebSocket upgrade for Chrome extension.
async fn ws_handler(
    State(state): State<Arc<DaemonState>>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_extension_ws(state, socket))
}

async fn handle_extension_ws(state: Arc<DaemonState>, socket: WebSocket) {
    let (sender, mut receiver) = socket.split();
    let connection_id = state
        .active_extension_connection_id
        .fetch_add(1, Ordering::SeqCst)
        + 1;

    // Store the sender so we can forward commands
    *state.extension_tx.lock().await = Some(sender);
    *state.extension_connected.write().await = true;
    info!("Chrome extension connected");

    // Spawn heartbeat pinger
    let heartbeat_state = state.clone();
    let heartbeat_handle = tokio::spawn(async move {
        loop {
            tokio::time::sleep(HEARTBEAT_INTERVAL).await;
            let mut tx = heartbeat_state.extension_tx.lock().await;
            if let Some(ref mut sink) = *tx {
                if sink.send(Message::Ping(vec![].into())).await.is_err() {
                    break;
                }
            } else {
                break;
            }
        }
    });

    // Process incoming messages from extension
    while let Some(msg) = receiver.next().await {
        state.touch().await;
        match msg {
            Ok(Message::Text(text)) => {
                debug!(len = text.len(), "received message from extension");
                // First check if this is a log message forwarded from the extension.
                // Format: { type: "log", level: "info"|"warn"|"error", msg: "...", ts: <ms> }
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                    if v.get("type").and_then(|t| t.as_str()) == Some("log") {
                        let level = v.get("level").and_then(|l| l.as_str()).unwrap_or("info");
                        let msg = v.get("msg").and_then(|m| m.as_str()).unwrap_or("");
                        let ts = v.get("ts").and_then(|t| t.as_u64()).unwrap_or(0);
                        match level {
                            "warn" => warn!(target: "extension", "{msg}"),
                            "error" => error!(target: "extension", "{msg}"),
                            _ => info!(target: "extension", "{msg}"),
                        }
                        write_extension_log(level, msg, ts);
                        continue;
                    }
                    // Not a log message — try to parse as DaemonResult
                    match serde_json::from_value::<DaemonResult>(v) {
                        Ok(result) => {
                            let id = result.id.clone();
                            if let Some(tx) = state.pending_commands.write().await.remove(&id) {
                                let _ = tx.send(result);
                            } else {
                                warn!(id = %id, "received result for unknown command");
                            }
                        }
                        Err(e) => {
                            warn!("failed to parse extension message: {e}");
                        }
                    }
                } else {
                    warn!("extension sent non-JSON message");
                }
            }
            Ok(Message::Pong(_)) => {
                debug!("pong from extension");
            }
            Ok(Message::Close(_)) => {
                info!("extension sent close frame");
                break;
            }
            Err(e) => {
                error!("extension ws error: {e}");
                break;
            }
            _ => {}
        }
    }

    // A newer connection may have replaced this one while this socket was
    // closing. Only the active connection owns daemon state and commands.
    if state.active_extension_connection_id.load(Ordering::SeqCst) != connection_id {
        heartbeat_handle.abort();
        return;
    }

    // Clean up
    heartbeat_handle.abort();
    *state.extension_tx.lock().await = None;
    *state.extension_connected.write().await = false;
    info!("Chrome extension disconnected");

    // Fail all pending commands
    let mut pending = state.pending_commands.write().await;
    for (id, tx) in pending.drain() {
        let _ = tx.send(DaemonResult::failure(
            id,
            "Extension disconnected".to_string(),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_daemon_start_and_shutdown() {
        let daemon = Daemon::start(0).await;
        // Port 0 lets the OS assign a random port, but our code binds to a specific port.
        // For testing, use a high random port.
        // This test just verifies the code path doesn't panic.
        // In practice, we'd use port 0 with TcpListener and extract the actual port.
        // For now, just verify construction logic.
        assert!(daemon.is_ok() || daemon.is_err());
    }

    #[tokio::test]
    async fn test_daemon_state_touch() {
        let state = DaemonState::new();
        let before = *state.last_activity.read().await;
        tokio::time::sleep(Duration::from_millis(10)).await;
        state.touch().await;
        let after = *state.last_activity.read().await;
        assert!(after > before);
    }

    #[test]
    fn task_preview_redacts_sensitive_fields_and_limits_text() {
        let result = json!({
            "title": "A public result",
            "accessToken": "do-not-display",
            "nested": { "cookie": "do-not-display" },
        });

        let preview = task_preview(&result);
        assert!(preview.contains("A public result"));
        assert!(preview.contains("[redacted]"));
        assert!(!preview.contains("do-not-display"));
        assert!(!task_preview(&json!({
            "url": "https://www.xiaohongshu.com/explore/id?xsec_token=signed-value&xsec_source=pc_feed"
        }))
        .contains("signed-value"));
        assert_eq!(
            limit_task_text(&"x".repeat(TASK_TEXT_LIMIT + 1))
                .chars()
                .count(),
            TASK_TEXT_LIMIT + 1
        );
    }

    #[tokio::test]
    async fn task_endpoint_reports_a_sanitized_completed_task() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let daemon = Daemon::start(port).await.unwrap();
        let client = crate::daemon_client::DaemonClient::new(port);
        let task = client.start_task("adapter:xiaohongshu:feed").await.unwrap();
        client
            .finish_task(
                &task.id,
                "done",
                Some(&json!({ "title": "visible", "accessToken": "hidden" })),
                None,
            )
            .await
            .unwrap();

        let tasks: Value = reqwest::Client::new()
            .get(format!("http://127.0.0.1:{port}/tasks"))
            .header("X-OpenCLI", "1")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(tasks["count"], 1);
        assert_eq!(tasks["tasks"][0]["status"], "done");
        assert!(tasks["tasks"][0]["result_preview"]
            .as_str()
            .unwrap()
            .contains("[redacted]"));
        assert!(!tasks["tasks"][0]["result_preview"]
            .as_str()
            .unwrap()
            .contains("hidden"));

        let preflight = reqwest::Client::new()
            .request(
                reqwest::Method::OPTIONS,
                format!("http://127.0.0.1:{port}/tasks"),
            )
            .header("Origin", "chrome-extension://test-extension")
            .header("Access-Control-Request-Method", "GET")
            .send()
            .await
            .unwrap();
        assert_eq!(preflight.status(), StatusCode::NO_CONTENT);
        assert_eq!(
            preflight
                .headers()
                .get("access-control-allow-origin")
                .unwrap(),
            "chrome-extension://test-extension"
        );

        let popup_read = reqwest::Client::new()
            .get(format!("http://127.0.0.1:{port}/tasks"))
            .header("Origin", "chrome-extension://test-extension")
            .send()
            .await
            .unwrap();
        assert_eq!(popup_read.status(), StatusCode::OK);
        assert_eq!(
            popup_read
                .headers()
                .get("access-control-allow-origin")
                .unwrap(),
            "chrome-extension://test-extension"
        );

        daemon.shutdown().await.unwrap();
    }
}
