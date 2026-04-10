use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

use async_trait::async_trait;
use opencli_rs_core::{CliError, IPage, ScreenshotOptions, SnapshotOptions};
use serde_json::Value;
use tracing::info;

use crate::step_registry::{StepHandler, StepRegistry};
use crate::template::{render_template_str, TemplateContext};

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn require_page(page: &Option<Arc<dyn IPage>>) -> Result<Arc<dyn IPage>, CliError> {
    page.clone()
        .ok_or_else(|| CliError::pipeline("browser step requires an active page"))
}

fn default_ctx(data: &Value, args: &HashMap<String, Value>) -> TemplateContext {
    TemplateContext {
        args: args.clone(),
        data: data.clone(),
        item: Value::Null,
        index: 0,
    }
}

fn render_str_param(
    params: &Value,
    data: &Value,
    args: &HashMap<String, Value>,
) -> Result<String, CliError> {
    let raw = params
        .as_str()
        .ok_or_else(|| CliError::pipeline("expected a string parameter"))?;
    let ctx = default_ctx(data, args);
    let rendered = render_template_str(raw, &ctx)?;
    rendered
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| CliError::pipeline("rendered template is not a string"))
}

/// Resolve template variables in a path string.
/// Supports: {ts} (unix timestamp), {ts_ms} (milliseconds), {step} (step index).
fn resolve_dump_path(path_tpl: &str, step_index: usize) -> String {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    path_tpl
        .replace("{ts}", &now.as_secs().to_string())
        .replace("{ts_ms}", &now.as_millis().to_string())
        .replace("{step}", &step_index.to_string())
}

/// Dump a JSON value to a file. Creates parent directories if needed.
/// - Arrays/Objects → pretty-printed JSON
/// - Primitives (string, number, bool, null) → raw value written as-is
/// Silently skips on errors (just logs a warning).
fn dump_value_to_file(value: &Value, path: &Path) {
    let content = match value {
        Value::Object(_) | Value::Array(_) => {
            serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".to_string())
        }
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
    };
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            let _ = fs::create_dir_all(parent);
        }
    }
    match fs::write(path, &content) {
        Ok(_) => info!(
            path = path.to_string_lossy().as_ref(),
            "Dumped raw data to file"
        ),
        Err(e) => tracing::warn!(path = %path.display(), err = %e, "Failed to dump raw data"),
    }
}

fn api_dump_enabled() -> bool {
    matches!(
        std::env::var("OPENCLI_API_DUMP"),
        Ok(v) if matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON")
    )
}

fn sanitize_dump_part(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    while out.contains("__") {
        out = out.replace("__", "_");
    }
    out.trim_matches('_').chars().take(80).collect()
}

fn dump_api_response(step: &str, url: &str, value: &Value) {
    if !api_dump_enabled() {
        return;
    }
    let base_dir = std::env::var("OPENCLI_API_DUMP_DIR")
        .unwrap_or_else(|_| "./data/api-dumps".to_string());
    let step_part = sanitize_dump_part(step);
    let url_part = sanitize_dump_part(url);
    let path = format!("{base_dir}/{step_part}_{url_part}_{{ts_ms}}.json");
    let resolved_path = resolve_dump_path(&path, 0);
    dump_value_to_file(value, Path::new(&resolved_path));
}

// ---------------------------------------------------------------------------
// NavigateStep
// ---------------------------------------------------------------------------

pub struct NavigateStep;

#[async_trait]
impl StepHandler for NavigateStep {
    fn name(&self) -> &'static str {
        "navigate"
    }

    fn is_browser_step(&self) -> bool {
        true
    }

    async fn execute(
        &self,
        page: Option<Arc<dyn IPage>>,
        params: &Value,
        data: &Value,
        args: &HashMap<String, Value>,
    ) -> Result<Value, CliError> {
        let pg = require_page(&page)?;
        let ctx = default_ctx(data, args);

        let (url, settle_ms) = match params {
            // navigate: "https://example.com"
            Value::String(s) => {
                let rendered = render_template_str(s, &ctx)?;
                let url = rendered.as_str().unwrap_or("").to_string();
                (url, None)
            }
            // navigate: { url: "...", settleMs: 2000 }
            Value::Object(obj) => {
                let url_val = obj
                    .get("url")
                    .ok_or_else(|| CliError::pipeline("navigate object requires 'url' field"))?;
                let url_str = url_val
                    .as_str()
                    .ok_or_else(|| CliError::pipeline("navigate 'url' must be a string"))?;
                let rendered = render_template_str(url_str, &ctx)?;
                let url = rendered.as_str().unwrap_or("").to_string();
                let settle = obj.get("settleMs").and_then(|v| v.as_u64());
                (url, settle)
            }
            _ => {
                return Err(CliError::pipeline(
                    "navigate expects a string URL or {url, settleMs} object",
                ))
            }
        };

        pg.goto(&url, None).await?;

        // Wait for page to settle if specified
        if let Some(ms) = settle_ms {
            tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
        }

        Ok(data.clone())
    }
}

// ---------------------------------------------------------------------------
// ClickStep
// ---------------------------------------------------------------------------

pub struct ClickStep;

#[async_trait]
impl StepHandler for ClickStep {
    fn name(&self) -> &'static str {
        "click"
    }

    fn is_browser_step(&self) -> bool {
        true
    }

    async fn execute(
        &self,
        page: Option<Arc<dyn IPage>>,
        params: &Value,
        data: &Value,
        args: &HashMap<String, Value>,
    ) -> Result<Value, CliError> {
        let pg = require_page(&page)?;
        let selector = render_str_param(params, data, args)?;
        pg.click(&selector).await?;
        Ok(data.clone())
    }
}

// ---------------------------------------------------------------------------
// TypeStep
// ---------------------------------------------------------------------------

pub struct TypeStep;

#[async_trait]
impl StepHandler for TypeStep {
    fn name(&self) -> &'static str {
        "type"
    }

    fn is_browser_step(&self) -> bool {
        true
    }

    async fn execute(
        &self,
        page: Option<Arc<dyn IPage>>,
        params: &Value,
        data: &Value,
        args: &HashMap<String, Value>,
    ) -> Result<Value, CliError> {
        let pg = require_page(&page)?;
        let ctx = default_ctx(data, args);

        let (selector, text) = match params {
            Value::Object(obj) => {
                let sel_raw = obj
                    .get("selector")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| CliError::pipeline("type: missing 'selector' field"))?;
                let text_raw = obj
                    .get("text")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| CliError::pipeline("type: missing 'text' field"))?;

                let sel = render_template_str(sel_raw, &ctx)?;
                let txt = render_template_str(text_raw, &ctx)?;
                (
                    sel.as_str()
                        .ok_or_else(|| {
                            CliError::pipeline("type: rendered selector is not a string")
                        })?
                        .to_string(),
                    txt.as_str()
                        .ok_or_else(|| CliError::pipeline("type: rendered text is not a string"))?
                        .to_string(),
                )
            }
            _ => {
                return Err(CliError::pipeline(
                    "type: params must be an object with 'selector' and 'text'",
                ))
            }
        };

        pg.type_text(&selector, &text).await?;
        Ok(data.clone())
    }
}

// ---------------------------------------------------------------------------
// WaitStep
// ---------------------------------------------------------------------------

pub struct WaitStep;

#[async_trait]
impl StepHandler for WaitStep {
    fn name(&self) -> &'static str {
        "wait"
    }

    fn is_browser_step(&self) -> bool {
        true
    }

    async fn execute(
        &self,
        page: Option<Arc<dyn IPage>>,
        params: &Value,
        data: &Value,
        _args: &HashMap<String, Value>,
    ) -> Result<Value, CliError> {
        let pg = require_page(&page)?;

        match params {
            // wait: 2 (seconds — matching original opencli convention)
            Value::Number(n) => {
                let secs = n.as_f64().unwrap_or(1.0);
                let ms = (secs * 1000.0) as u64;
                pg.wait_for_timeout(ms).await?;
            }
            Value::Object(obj) => {
                if let Some(time_val) = obj.get("time") {
                    let secs = time_val.as_f64().unwrap_or(1.0);
                    let ms = (secs * 1000.0) as u64;
                    pg.wait_for_timeout(ms).await?;
                } else if let Some(sel_val) = obj.get("selector") {
                    let selector = sel_val
                        .as_str()
                        .ok_or_else(|| CliError::pipeline("wait: 'selector' must be a string"))?;
                    pg.wait_for_selector(selector, None).await?;
                } else if let Some(text_val) = obj.get("text") {
                    // Wait for text by using wait_for_selector with an XPath-like approach
                    // Since IPage doesn't have wait_for_text, we use evaluate in a polling loop
                    let text = text_val
                        .as_str()
                        .ok_or_else(|| CliError::pipeline("wait: 'text' must be a string"))?;
                    let js = format!(
                        r#"new Promise((resolve, reject) => {{
                            const timeout = setTimeout(() => reject(new Error('Timeout waiting for text')), 30000);
                            const check = () => {{
                                if (document.body.innerText.includes({})) {{
                                    clearTimeout(timeout);
                                    resolve(true);
                                }} else {{
                                    requestAnimationFrame(check);
                                }}
                            }};
                            check();
                        }})"#,
                        serde_json::to_string(text).unwrap_or_default()
                    );
                    pg.evaluate(&js).await?;
                } else {
                    return Err(CliError::pipeline(
                        "wait: object must have 'time', 'selector', or 'text'",
                    ));
                }
            }
            _ => {
                return Err(CliError::pipeline(
                    "wait: params must be a number or object",
                ))
            }
        }

        Ok(data.clone())
    }
}

// ---------------------------------------------------------------------------
// PressStep
// ---------------------------------------------------------------------------

pub struct PressStep;

#[async_trait]
impl StepHandler for PressStep {
    fn name(&self) -> &'static str {
        "press"
    }

    fn is_browser_step(&self) -> bool {
        true
    }

    async fn execute(
        &self,
        page: Option<Arc<dyn IPage>>,
        params: &Value,
        data: &Value,
        args: &HashMap<String, Value>,
    ) -> Result<Value, CliError> {
        let pg = require_page(&page)?;
        let key = render_str_param(params, data, args)?;
        // Use evaluate to dispatch keyboard events since IPage has no press_key method
        let js = format!(
            r#"document.dispatchEvent(new KeyboardEvent('keydown', {{ key: {key}, bubbles: true }}));
               document.dispatchEvent(new KeyboardEvent('keyup', {{ key: {key}, bubbles: true }}));"#,
            key = serde_json::to_string(&key).unwrap_or_default()
        );
        pg.evaluate(&js).await?;
        Ok(data.clone())
    }
}

// ---------------------------------------------------------------------------
// EvaluateStep
// ---------------------------------------------------------------------------

pub struct EvaluateStep;

#[async_trait]
impl StepHandler for EvaluateStep {
    fn name(&self) -> &'static str {
        "evaluate"
    }

    fn is_browser_step(&self) -> bool {
        true
    }

    async fn execute(
        &self,
        page: Option<Arc<dyn IPage>>,
        params: &Value,
        data: &Value,
        args: &HashMap<String, Value>,
    ) -> Result<Value, CliError> {
        let pg = require_page(&page)?;

        // Support both string form (js code) and object form {js?, format, path}
        // In object form, the js code is the first string value found (js field or any string field).
        // Other fields are treated as options: format, path.
        let (js_code, raw_dump) = match params {
            Value::String(s) => (s.clone(), None),
            Value::Object(obj) => {
                // Find the JS code: prefer explicit "js" field, fall back to first string value
                let js = obj
                    .get("js")
                    .and_then(|v| v.as_str())
                    .or_else(|| obj.values().find_map(|v| v.as_str()))
                    .ok_or_else(|| {
                        CliError::pipeline("evaluate: object form requires a js code string")
                    })?;

                let raw_dump = if obj
                    .get("format")
                    .and_then(|v| v.as_str())
                    .map(|f| f == "raw")
                    .unwrap_or(false)
                {
                    let path = obj
                        .get("path")
                        .and_then(|v| v.as_str())
                        .unwrap_or("./data/raw_{ts}.json");
                    Some(path.to_string())
                } else {
                    None
                };
                (js.to_string(), raw_dump)
            }
            _ => {
                return Err(CliError::pipeline(
                    "evaluate: params must be a string or {js?, format?, path?} object",
                ))
            }
        };

        let js = render_str_param(&Value::String(js_code), data, args)?;

        // Inject `args` and `data` as local variables so JS code can reference them
        // directly (e.g. `args.query`, `args.limit`) without ${{ }} template syntax.
        // This matches the original opencli behavior.
        let args_json = serde_json::to_string(args).unwrap_or("{}".to_string());
        let data_json = serde_json::to_string(data).unwrap_or("null".to_string());
        let wrapped_js = format!(
            "(function() {{ const args = {}; const data = {}; return ({}); }})()",
            args_json, data_json, js
        );

        let result = pg.evaluate(&wrapped_js).await?;

        // Dump raw data if format=raw is specified
        if let Some(path_tpl) = raw_dump {
            let resolved_path = resolve_dump_path(&path_tpl, 0);
            let path = Path::new(&resolved_path);
            dump_value_to_file(&result, path);
        }

        Ok(result)
    }
}

// ---------------------------------------------------------------------------
// SnapshotStep
// ---------------------------------------------------------------------------

pub struct SnapshotStep;

#[async_trait]
impl StepHandler for SnapshotStep {
    fn name(&self) -> &'static str {
        "snapshot"
    }

    fn is_browser_step(&self) -> bool {
        true
    }

    async fn execute(
        &self,
        page: Option<Arc<dyn IPage>>,
        params: &Value,
        data: &Value,
        _args: &HashMap<String, Value>,
    ) -> Result<Value, CliError> {
        let pg = require_page(&page)?;

        let opts = match params {
            Value::Object(obj) => {
                let selector = obj
                    .get("selector")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                let include_hidden = obj
                    .get("include_hidden")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                Some(SnapshotOptions {
                    selector,
                    include_hidden,
                })
            }
            Value::Null => None,
            _ => None,
        };

        let result = pg.snapshot(opts).await?;
        if result.is_null() {
            Ok(data.clone())
        } else {
            Ok(result)
        }
    }
}

// ---------------------------------------------------------------------------
// ScreenshotStep
// ---------------------------------------------------------------------------

pub struct ScreenshotStep;

#[async_trait]
impl StepHandler for ScreenshotStep {
    fn name(&self) -> &'static str {
        "screenshot"
    }

    fn is_browser_step(&self) -> bool {
        true
    }

    async fn execute(
        &self,
        page: Option<Arc<dyn IPage>>,
        params: &Value,
        _data: &Value,
        _args: &HashMap<String, Value>,
    ) -> Result<Value, CliError> {
        let pg = require_page(&page)?;

        let opts = match params {
            Value::Object(obj) => {
                let full_page = obj
                    .get("full_page")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let selector = obj
                    .get("selector")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                let path = obj.get("path").and_then(|v| v.as_str()).map(String::from);
                Some(ScreenshotOptions {
                    path,
                    full_page,
                    selector,
                })
            }
            Value::Null => None,
            _ => None,
        };

        let bytes = pg.screenshot(opts).await?;
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        Ok(Value::String(b64))
    }
}

// ---------------------------------------------------------------------------
// ScrollStep
// ---------------------------------------------------------------------------

pub struct ScrollStep;

#[async_trait]
impl StepHandler for ScrollStep {
    fn name(&self) -> &'static str {
        "scroll"
    }

    fn is_browser_step(&self) -> bool {
        true
    }

    async fn execute(
        &self,
        page: Option<Arc<dyn IPage>>,
        params: &Value,
        data: &Value,
        args: &HashMap<String, Value>,
    ) -> Result<Value, CliError> {
        let pg = require_page(&page)?;

        match params {
            // scroll: 3  (number of scrolls)
            Value::Number(n) => {
                let count = n.as_u64().unwrap_or(3) as u32;
                pg.auto_scroll(Some(opencli_rs_core::AutoScrollOptions {
                    max_scrolls: Some(count),
                    delay_ms: Some(300),
                    ..Default::default()
                }))
                .await?;
            }
            // scroll: { direction: "down", count: 5, delay: 500 }
            Value::Object(obj) => {
                let count = obj.get("count").and_then(|v| v.as_u64()).unwrap_or(3) as u32;
                let delay = obj.get("delay").and_then(|v| v.as_u64()).unwrap_or(300);
                pg.auto_scroll(Some(opencli_rs_core::AutoScrollOptions {
                    max_scrolls: Some(count),
                    delay_ms: Some(delay),
                    ..Default::default()
                }))
                .await?;
            }
            // scroll: "down" or template string
            Value::String(_) => {
                let ctx = default_ctx(data, args);
                let rendered = render_template_str(params.as_str().unwrap_or("3"), &ctx)?;
                let count = rendered
                    .as_u64()
                    .or_else(|| rendered.as_str().and_then(|s| s.parse().ok()))
                    .unwrap_or(3) as u32;
                pg.auto_scroll(Some(opencli_rs_core::AutoScrollOptions {
                    max_scrolls: Some(count),
                    delay_ms: Some(300),
                    ..Default::default()
                }))
                .await?;
            }
            // scroll: null → default 3 scrolls
            _ => {
                pg.auto_scroll(Some(opencli_rs_core::AutoScrollOptions {
                    max_scrolls: Some(3),
                    delay_ms: Some(300),
                    ..Default::default()
                }))
                .await?;
            }
        }

        Ok(data.clone())
    }
}

// ---------------------------------------------------------------------------
// CollectStep — collect intercepted requests and parse with JS function
// ---------------------------------------------------------------------------

pub struct CollectStep;

#[async_trait]
impl StepHandler for CollectStep {
    fn name(&self) -> &'static str {
        "collect"
    }

    fn is_browser_step(&self) -> bool {
        true
    }

    async fn execute(
        &self,
        page: Option<Arc<dyn IPage>>,
        params: &Value,
        _data: &Value,
        args: &HashMap<String, Value>,
    ) -> Result<Value, CliError> {
        let pg = require_page(&page)?;

        // Get the parse function from params
        let parse_fn = params
            .get("parse")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                CliError::pipeline("collect step requires a 'parse' field with a JS function")
            })?;

        // Get intercepted data directly from browser (raw JSON, not typed structs)
        // and run the parse function on it — all in one evaluate call.
        let args_json = serde_json::to_string(args).unwrap_or("{}".to_string());
        let js = format!(
            r#"(() => {{
  const args = {args_json};
  const requests = window.__opencli_intercepted || [];
  window.__opencli_intercepted = [];
  const parseFn = {parse_fn};
  return parseFn(requests);
}})()"#
        );

        pg.evaluate(&js).await
    }
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// BgFetchStep — run a fetch in the extension service worker
// ---------------------------------------------------------------------------
// Extension opens a background tab to establish CDP connection, then runs fetch
// with cookies injected. No visible window (extension creates background tab in
// user's existing Chrome window or minimized fallback window).

struct BgFetchStep;

fn render_str(
    params: &Value,
    key: &str,
    ctx: &TemplateContext,
) -> Result<Option<String>, CliError> {
    match params.get(key).and_then(|v| v.as_str()) {
        Some(s) => match render_template_str(s, ctx)? {
            Value::String(s) => Ok(Some(s)),
            other => Ok(Some(other.to_string())),
        },
        None => Ok(None),
    }
}

#[async_trait]
impl StepHandler for BgFetchStep {
    fn name(&self) -> &'static str {
        "bg_fetch"
    }

    async fn execute(
        &self,
        page: Option<Arc<dyn IPage>>,
        params: &Value,
        data: &Value,
        args: &HashMap<String, Value>,
    ) -> Result<Value, CliError> {
        let page = require_page(&page)?;
        let ctx = default_ctx(data, args);

        let url = render_str(params, "url", &ctx)?
            .ok_or_else(|| CliError::pipeline("bg_fetch: missing required field 'url'"))?;
        let cookie_url = render_str(params, "cookie_url", &ctx)?;
        let method = params
            .get("method")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let request_headers: Option<std::collections::HashMap<String, String>> = params
            .get("headers")
            .and_then(|v| v.as_object())
            .map(|obj| {
                obj.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            });
        let body = params
            .get("body")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let result = page
            .bg_fetch(
                &url,
                cookie_url.as_deref(),
                method.as_deref(),
                request_headers,
                body.as_deref(),
            )
            .await?;

        dump_api_response("bg_fetch", &url, &result);

        // Return { status, body } — let pipeline select the body
        Ok(result)
    }
}

pub fn register_browser_steps(registry: &mut StepRegistry) {
    registry.register(Arc::new(NavigateStep));
    registry.register(Arc::new(ClickStep));
    registry.register(Arc::new(TypeStep));
    registry.register(Arc::new(WaitStep));
    registry.register(Arc::new(PressStep));
    registry.register(Arc::new(EvaluateStep));
    registry.register(Arc::new(SnapshotStep));
    registry.register(Arc::new(ScreenshotStep));
    registry.register(Arc::new(ScrollStep));
    registry.register(Arc::new(CollectStep));
    registry.register(Arc::new(BgFetchStep));
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use opencli_rs_core::WaitOptions;
    use serde_json::json;

    fn empty_args() -> HashMap<String, Value> {
        HashMap::new()
    }

    // Mock IPage for testing
    struct MockPage {
        goto_url: std::sync::Mutex<Option<String>>,
        evaluate_result: Value,
    }

    impl MockPage {
        fn new(evaluate_result: Value) -> Self {
            Self {
                goto_url: std::sync::Mutex::new(None),
                evaluate_result,
            }
        }
    }

    #[async_trait]
    impl IPage for MockPage {
        async fn goto(
            &self,
            url: &str,
            _options: Option<opencli_rs_core::GotoOptions>,
        ) -> Result<(), CliError> {
            *self.goto_url.lock().unwrap() = Some(url.to_string());
            Ok(())
        }
        async fn url(&self) -> Result<String, CliError> {
            Ok("https://example.com".to_string())
        }
        async fn title(&self) -> Result<String, CliError> {
            Ok("Mock".to_string())
        }
        async fn content(&self) -> Result<String, CliError> {
            Ok("<html></html>".to_string())
        }
        async fn evaluate(&self, _expression: &str) -> Result<Value, CliError> {
            Ok(self.evaluate_result.clone())
        }
        async fn wait_for_selector(
            &self,
            _selector: &str,
            _options: Option<WaitOptions>,
        ) -> Result<(), CliError> {
            Ok(())
        }
        async fn wait_for_navigation(&self, _options: Option<WaitOptions>) -> Result<(), CliError> {
            Ok(())
        }
        async fn wait_for_timeout(&self, _ms: u64) -> Result<(), CliError> {
            Ok(())
        }
        async fn click(&self, _selector: &str) -> Result<(), CliError> {
            Ok(())
        }
        async fn type_text(&self, _selector: &str, _text: &str) -> Result<(), CliError> {
            Ok(())
        }
        async fn cookies(
            &self,
            _options: Option<opencli_rs_core::CookieOptions>,
        ) -> Result<Vec<opencli_rs_core::Cookie>, CliError> {
            Ok(vec![])
        }
        async fn set_cookies(
            &self,
            _cookies: Vec<opencli_rs_core::Cookie>,
        ) -> Result<(), CliError> {
            Ok(())
        }
        async fn screenshot(
            &self,
            _options: Option<ScreenshotOptions>,
        ) -> Result<Vec<u8>, CliError> {
            Ok(vec![0x89, 0x50, 0x4E, 0x47]) // PNG magic bytes
        }
        async fn snapshot(&self, _options: Option<SnapshotOptions>) -> Result<Value, CliError> {
            Ok(json!({"tree": "snapshot"}))
        }
        async fn auto_scroll(
            &self,
            _options: Option<opencli_rs_core::AutoScrollOptions>,
        ) -> Result<(), CliError> {
            Ok(())
        }
        async fn tabs(&self) -> Result<Vec<opencli_rs_core::TabInfo>, CliError> {
            Ok(vec![])
        }
        async fn switch_tab(&self, _tab_id: &str) -> Result<(), CliError> {
            Ok(())
        }
        async fn close(&self) -> Result<(), CliError> {
            Ok(())
        }
        async fn intercept_requests(&self, _url_pattern: &str) -> Result<(), CliError> {
            Ok(())
        }
        async fn get_intercepted_requests(
            &self,
        ) -> Result<Vec<opencli_rs_core::InterceptedRequest>, CliError> {
            Ok(vec![])
        }
        async fn get_network_requests(
            &self,
        ) -> Result<Vec<opencli_rs_core::NetworkRequest>, CliError> {
            Ok(vec![])
        }
    }

    #[tokio::test]
    async fn test_all_browser_steps_register() {
        let mut registry = StepRegistry::new();
        register_browser_steps(&mut registry);
        assert!(registry.get("navigate").is_some());
        assert!(registry.get("click").is_some());
        assert!(registry.get("type").is_some());
        assert!(registry.get("wait").is_some());
        assert!(registry.get("press").is_some());
        assert!(registry.get("evaluate").is_some());
        assert!(registry.get("snapshot").is_some());
        assert!(registry.get("screenshot").is_some());
    }

    #[tokio::test]
    async fn test_navigate_step() {
        let mock = Arc::new(MockPage::new(json!(null)));
        let step = NavigateStep;
        let result = step
            .execute(
                Some(mock.clone()),
                &json!("https://example.com"),
                &json!({"key": "value"}),
                &empty_args(),
            )
            .await
            .unwrap();
        assert_eq!(result, json!({"key": "value"}));
        assert_eq!(
            *mock.goto_url.lock().unwrap(),
            Some("https://example.com".to_string())
        );
    }

    #[tokio::test]
    async fn test_evaluate_step() {
        let mock = Arc::new(MockPage::new(json!({"items": [1, 2, 3]})));
        let step = EvaluateStep;
        let result = step
            .execute(
                Some(mock),
                &json!("document.querySelectorAll('.item')"),
                &json!(null),
                &empty_args(),
            )
            .await
            .unwrap();
        assert_eq!(result, json!({"items": [1, 2, 3]}));
    }

    #[tokio::test]
    async fn test_browser_step_requires_page() {
        let step = NavigateStep;
        let result = step
            .execute(
                None,
                &json!("https://example.com"),
                &json!(null),
                &empty_args(),
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_all_browser_steps_are_browser_steps() {
        assert!(NavigateStep.is_browser_step());
        assert!(ClickStep.is_browser_step());
        assert!(TypeStep.is_browser_step());
        assert!(WaitStep.is_browser_step());
        assert!(PressStep.is_browser_step());
        assert!(EvaluateStep.is_browser_step());
        assert!(SnapshotStep.is_browser_step());
        assert!(ScreenshotStep.is_browser_step());
    }

    #[tokio::test]
    async fn test_wait_step_with_time() {
        let mock = Arc::new(MockPage::new(json!(null)));
        let step = WaitStep;
        let result = step
            .execute(Some(mock), &json!(1000), &json!("data"), &empty_args())
            .await
            .unwrap();
        assert_eq!(result, json!("data"));
    }

    #[tokio::test]
    async fn test_snapshot_step() {
        let mock = Arc::new(MockPage::new(json!(null)));
        let step = SnapshotStep;
        let result = step
            .execute(Some(mock), &json!(null), &json!(null), &empty_args())
            .await
            .unwrap();
        assert_eq!(result, json!({"tree": "snapshot"}));
    }
}
