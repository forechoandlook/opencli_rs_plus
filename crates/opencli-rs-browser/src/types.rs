use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonCommand {
    pub id: String,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Navigation readiness: "commit" resolves after the target URL commits;
    /// the default waits for the page load event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait_until: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// HTTP method for bg_fetch (default: GET)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    /// Extra request headers for bg_fetch
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_headers: Option<HashMap<String, String>>,
    /// Request body for bg_fetch
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// URL to extract cookies from for bg_fetch (defaults to url)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cookie_url: Option<String>,
    /// CSS selector for a browser-native file upload action.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    /// Absolute local paths to place into the selected file input.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_paths: Option<Vec<String>>,
}

impl DaemonCommand {
    pub fn new(action: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            action: action.into(),
            code: None,
            url: None,
            wait_until: None,
            workspace: None,
            tab_id: None,
            format: None,
            method: None,
            request_headers: None,
            body: None,
            cookie_url: None,
            selector: None,
            file_paths: None,
        }
    }

    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    pub fn with_wait_until(mut self, wait_until: impl Into<String>) -> Self {
        self.wait_until = Some(wait_until.into());
        self
    }

    pub fn with_workspace(mut self, workspace: impl Into<String>) -> Self {
        self.workspace = Some(workspace.into());
        self
    }

    pub fn with_tab_id(mut self, tab_id: u64) -> Self {
        self.tab_id = Some(tab_id);
        self
    }

    pub fn with_format(mut self, format: impl Into<String>) -> Self {
        self.format = Some(format.into());
        self
    }

    pub fn with_method(mut self, method: impl Into<String>) -> Self {
        self.method = Some(method.into());
        self
    }

    pub fn with_request_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.request_headers = Some(headers);
        self
    }

    pub fn with_body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }

    pub fn with_cookie_url(mut self, cookie_url: impl Into<String>) -> Self {
        self.cookie_url = Some(cookie_url.into());
        self
    }

    pub fn with_selector(mut self, selector: impl Into<String>) -> Self {
        self.selector = Some(selector.into());
        self
    }

    pub fn with_file_paths(mut self, paths: Vec<String>) -> Self {
        self.file_paths = Some(paths);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonResult {
    pub id: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl DaemonResult {
    pub fn success(id: String, data: Value) -> Self {
        Self {
            id,
            ok: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn failure(id: String, error: String) -> Self {
        Self {
            id,
            ok: false,
            data: None,
            error: Some(error),
        }
    }
}
