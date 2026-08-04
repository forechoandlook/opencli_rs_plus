use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use opencli_rs_core::{CliError, GotoOptions, IPage};
use serde_json::Value;

use crate::step_registry::{StepHandler, StepRegistry};
use crate::steps::dump::{dump_api_response, dump_value_to_file, resolve_dump_path};
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

        let (url, settle_ms, wait_until, reuse_current) = match params {
            // navigate: "https://example.com"
            Value::String(s) => {
                let rendered = render_template_str(s, &ctx)?;
                let url = rendered.as_str().unwrap_or("").to_string();
                (url, None, None, false)
            }
            // navigate: { url: "...", waitUntil: commit, reuseCurrent: true }
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
                let wait_until = obj
                    .get("waitUntil")
                    .or_else(|| obj.get("wait_until"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                let reuse_current = obj
                    .get("reuseCurrent")
                    .or_else(|| obj.get("reuse_current"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                (url, settle, wait_until, reuse_current)
            }
            _ => {
                return Err(CliError::pipeline(
                    "navigate expects a string URL or {url, settleMs} object",
                ))
            }
        };

        let current_url = if reuse_current {
            pg.url().await.unwrap_or_default()
        } else {
            String::new()
        };
        let already_at_target = reuse_current && same_document_url(&current_url, &url);
        if !already_at_target {
            let options = wait_until.map(|wait_until| GotoOptions {
                wait_until: Some(wait_until),
                timeout_ms: None,
            });
            pg.goto(&url, options).await?;
        }

        // Wait for page to settle if specified
        if let Some(ms) = settle_ms {
            tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
        }

        Ok(data.clone())
    }
}

fn same_document_url(current_url: &str, target_url: &str) -> bool {
    match (
        reqwest::Url::parse(current_url),
        reqwest::Url::parse(target_url),
    ) {
        (Ok(current), Ok(target)) => current == target,
        _ => current_url == target_url,
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
// The extension service worker fetches with browser-managed credentials. No
// visible user tab is used; browser automation stays in a minimized window.

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

async fn same_origin_fetch(
    page: Arc<dyn IPage>,
    context_url: &str,
    url: &str,
    method: Option<&str>,
    headers: Option<HashMap<String, String>>,
    body: Option<&str>,
) -> Result<Value, CliError> {
    // Fetch needs a first-party document, not a particular route. Reusing an
    // already-open route on the same origin retains cookies and avoids an
    // unnecessary navigation on every command invocation.
    let current_url = page.url().await.unwrap_or_default();
    let current_origin = reqwest::Url::parse(&current_url)
        .ok()
        .map(|url| url.origin());
    let context_origin = reqwest::Url::parse(context_url)
        .ok()
        .map(|url| url.origin());
    if current_origin != context_origin {
        page.goto(
            context_url,
            Some(GotoOptions {
                // API requests do not need images, tracking scripts, or other
                // subresources to finish loading before running fetch.
                wait_until: Some("commit".to_string()),
                timeout_ms: None,
            }),
        )
        .await?;
    }

    let init = serde_json::json!({
        "method": method.unwrap_or("GET"),
        "headers": headers.unwrap_or_default(),
        "body": body,
        "credentials": "include",
    });
    let url_json = serde_json::to_string(url).unwrap_or_else(|_| "\"\"".to_string());
    let init_json = serde_json::to_string(&init).unwrap_or_else(|_| "{}".to_string());
    let js = format!(
        r#"(async () => {{
  const response = await fetch({url_json}, {init_json});
  const text = await response.text();
  if (!response.ok) throw new Error(`HTTP ${{response.status}}: ${{text.slice(0, 400)}}`);
  let body = text;
  if ((response.headers.get('content-type') || '').includes('application/json')) {{
    try {{ body = JSON.parse(text); }} catch {{}}
  }}
  return {{ status: response.status, body }};
}})()"#,
    );
    page.evaluate(&js).await
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

        let same_origin = params
            .get("same_origin")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let context_url = cookie_url.as_deref().unwrap_or(&url);

        let result = if same_origin {
            same_origin_fetch(
                page.clone(),
                context_url,
                &url,
                method.as_deref(),
                request_headers,
                body.as_deref(),
            )
            .await?
        } else {
            match page
                .bg_fetch(
                    &url,
                    cookie_url.as_deref(),
                    method.as_deref(),
                    request_headers.clone(),
                    body.as_deref(),
                )
                .await
            {
                Ok(result) => result,
                Err(_) => {
                    // Some sites reject a Chrome extension service worker as a
                    // cross-origin caller even with host permissions. Retry from
                    // the same-origin page in OpenCLI's own minimized window so
                    // the browser can attach its normal first-party cookies.
                    same_origin_fetch(
                        page.clone(),
                        context_url,
                        &url,
                        method.as_deref(),
                        request_headers,
                        body.as_deref(),
                    )
                    .await?
                }
            }
        };

        dump_api_response("bg_fetch", &url, &result);

        // Return { status, body } — let pipeline select the body
        Ok(result)
    }
}

pub fn register_browser_steps(registry: &mut StepRegistry) {
    registry.register(Arc::new(NavigateStep));
    registry.register(Arc::new(WaitStep));
    registry.register(Arc::new(EvaluateStep));
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
        goto_options: std::sync::Mutex<Option<opencli_rs_core::GotoOptions>>,
        evaluate_script: std::sync::Mutex<Option<String>>,
        evaluate_result: Value,
        current_url: String,
    }

    impl MockPage {
        fn new(evaluate_result: Value) -> Self {
            Self {
                goto_url: std::sync::Mutex::new(None),
                goto_options: std::sync::Mutex::new(None),
                evaluate_script: std::sync::Mutex::new(None),
                evaluate_result,
                current_url: "https://example.com".to_string(),
            }
        }

        fn at_url(evaluate_result: Value, current_url: &str) -> Self {
            Self {
                current_url: current_url.to_string(),
                ..Self::new(evaluate_result)
            }
        }
    }

    #[async_trait]
    impl IPage for MockPage {
        async fn goto(
            &self,
            url: &str,
            options: Option<opencli_rs_core::GotoOptions>,
        ) -> Result<(), CliError> {
            *self.goto_url.lock().unwrap() = Some(url.to_string());
            *self.goto_options.lock().unwrap() = options;
            Ok(())
        }
        async fn url(&self) -> Result<String, CliError> {
            Ok(self.current_url.clone())
        }
        async fn title(&self) -> Result<String, CliError> {
            Ok("Mock".to_string())
        }
        async fn content(&self) -> Result<String, CliError> {
            Ok("<html></html>".to_string())
        }
        async fn evaluate(&self, expression: &str) -> Result<Value, CliError> {
            *self.evaluate_script.lock().unwrap() = Some(expression.to_string());
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
            _options: Option<opencli_rs_core::ScreenshotOptions>,
        ) -> Result<Vec<u8>, CliError> {
            Ok(vec![0x89, 0x50, 0x4E, 0x47]) // PNG magic bytes
        }
        async fn snapshot(
            &self,
            _options: Option<opencli_rs_core::SnapshotOptions>,
        ) -> Result<Value, CliError> {
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
        assert!(registry.get("wait").is_some());
        assert!(registry.get("evaluate").is_some());
        assert!(registry.get("scroll").is_some());
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
    async fn navigate_step_reuses_the_exact_cached_route_with_commit_wait() {
        let mock = Arc::new(MockPage::at_url(
            json!(null),
            "https://www.xiaohongshu.com/explore",
        ));
        let step = NavigateStep;
        step.execute(
            Some(mock.clone()),
            &json!({
                "url": "https://www.xiaohongshu.com/explore",
                "waitUntil": "commit",
                "reuseCurrent": true,
            }),
            &json!(null),
            &empty_args(),
        )
        .await
        .unwrap();

        assert_eq!(*mock.goto_url.lock().unwrap(), None);
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
        assert!(WaitStep.is_browser_step());
        assert!(EvaluateStep.is_browser_step());
        assert!(ScrollStep.is_browser_step());
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
    async fn bg_fetch_falls_back_to_a_same_origin_page_request() {
        let mock = Arc::new(MockPage::new(json!({
            "status": 200,
            "body": { "data": ["ok"] },
        })));
        let step = BgFetchStep;
        let result = step
            .execute(
                Some(mock.clone()),
                &json!({
                    "url": "https://www.zhihu.com/api/v3/feed/topstory/hot-lists/total?limit=1",
                    "cookie_url": "https://www.zhihu.com",
                }),
                &json!(null),
                &empty_args(),
            )
            .await
            .unwrap();

        assert_eq!(result["body"]["data"][0], "ok");
        assert_eq!(
            *mock.goto_url.lock().unwrap(),
            Some("https://www.zhihu.com".to_string())
        );
        assert_eq!(
            mock.goto_options
                .lock()
                .unwrap()
                .as_ref()
                .and_then(|options| options.wait_until.as_deref()),
            Some("commit")
        );
        let script = mock.evaluate_script.lock().unwrap().clone().unwrap();
        assert!(script.contains("credentials\":\"include"));
        assert!(script.contains("https://www.zhihu.com/api/v3/feed"));
    }

    #[tokio::test]
    async fn same_origin_fetch_reuses_any_route_on_the_same_origin() {
        let mock = Arc::new(MockPage::at_url(
            json!({ "status": 200, "body": { "data": ["ok"] } }),
            "https://www.zhihu.com/question/123",
        ));

        let result = same_origin_fetch(
            mock.clone(),
            "https://www.zhihu.com",
            "https://www.zhihu.com/api/v3/feed/topstory/hot-lists/total?limit=1",
            None,
            None,
            None,
        )
        .await
        .unwrap();

        assert_eq!(result["body"]["data"][0], "ok");
        assert_eq!(*mock.goto_url.lock().unwrap(), None);
    }
}
