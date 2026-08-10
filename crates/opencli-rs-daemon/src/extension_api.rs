//! Local HTTP API used by the OpenCLI browser extension.
//!
//! This server deliberately lives beside the opencli daemon, rather than in
//! browser-daemon: adapter discovery and path matching are daemon concerns;
//! browser-daemon remains a CDP/WebSocket relay only.

use crate::socket::SocketState;
use anyhow::Result;
use axum::{
    extract::{Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use opencli_rs_core::ContextAction;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Arc};
use tokio::task::JoinHandle;
use url::Url;

pub const DEFAULT_EXTENSION_API_ADDR: &str = "127.0.0.1:10009";

#[derive(Debug, Deserialize)]
struct ActionsQuery {
    url: String,
}

/// Bind the loopback-only extension API before the daemon announces itself.
pub async fn start(addr: &str, state: Arc<SocketState>) -> Result<JoinHandle<()>> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let app = Router::new()
        .route(
            "/extension/actions",
            get(actions_handler).options(options_handler),
        )
        .with_state(state);
    Ok(tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, app).await {
            tracing::error!(error = %error, "Extension API server error");
        }
    }))
}

async fn options_handler(headers: HeaderMap) -> Response {
    extension_response(&headers, StatusCode::NO_CONTENT.into_response())
}

async fn actions_handler(
    State(state): State<Arc<SocketState>>,
    headers: HeaderMap,
    Query(query): Query<ActionsQuery>,
) -> Response {
    if !is_extension_origin(&headers) {
        return extension_response(
            &headers,
            error_response(StatusCode::FORBIDDEN, "Chrome extension origin required"),
        );
    }
    let Ok(url) = Url::parse(&query.url) else {
        return extension_response(
            &headers,
            error_response(StatusCode::BAD_REQUEST, "Invalid page URL"),
        );
    };
    if !matches!(url.scheme(), "http" | "https") {
        return extension_response(
            &headers,
            error_response(StatusCode::BAD_REQUEST, "Only HTTP(S) pages are supported"),
        );
    }

    let actions: Vec<Value> = state
        .adapter_manager
        .list_adapters()
        .await
        .into_iter()
        .filter(|entry| entry.enabled)
        .filter_map(|entry| {
            let context = entry.context.as_ref()?;
            let active_tab = context.active_tab.as_ref()?;
            let args = context_args(context, &query.url).ok()?;
            context_matches(entry.domain.as_deref(), context, &url).then(|| {
                json!({
                    "adapter": entry.full_name,
                    "title": context.title,
                    "description": entry.description,
                    "activeTab": active_tab,
                    "args": args,
                    "pipeline": active_tab.use_pipeline.then(|| entry.pipeline.clone()).flatten(),
                })
            })
        })
        .collect();
    tracing::info!(
        host = url.host_str().unwrap_or_default(),
        path = url.path(),
        action_count = actions.len(),
        "Context action query"
    );

    extension_response(
        &headers,
        Json(json!({ "url": query.url, "actions": actions })).into_response(),
    )
}

fn context_args(context: &ContextAction, current_url: &str) -> Result<HashMap<String, String>, ()> {
    context
        .args
        .iter()
        .map(|(name, source)| match source.as_str() {
            "current_url" => Ok((name.clone(), current_url.to_string())),
            _ => Err(()),
        })
        .collect()
}

fn context_matches(domain: Option<&str>, context: &ContextAction, url: &Url) -> bool {
    let Some(domain) = domain.map(|value| value.trim().trim_end_matches('.').to_ascii_lowercase())
    else {
        return false;
    };
    let Some(host) = url
        .host_str()
        .map(|value| value.trim_end_matches('.').to_ascii_lowercase())
    else {
        return false;
    };
    if host != domain && !host.ends_with(&format!(".{domain}")) {
        return false;
    }
    context.paths.is_empty()
        || context
            .paths
            .iter()
            .any(|pattern| glob_matches(pattern, url.path()))
}

fn glob_matches(pattern: &str, value: &str) -> bool {
    let mut remaining = value;
    let mut first = true;
    for part in pattern.split('*') {
        if part.is_empty() {
            continue;
        }
        let Some(index) = remaining.find(part) else {
            return false;
        };
        if first && !pattern.starts_with('*') && index != 0 {
            return false;
        }
        remaining = &remaining[index + part.len()..];
        first = false;
    }
    pattern.ends_with('*') || remaining.is_empty()
}

fn error_response(status: StatusCode, error: &str) -> Response {
    (status, Json(json!({ "error": error }))).into_response()
}

fn is_extension_origin(headers: &HeaderMap) -> bool {
    headers
        .get(header::ORIGIN)
        .and_then(|origin| origin.to_str().ok())
        .is_some_and(|origin| origin.starts_with("chrome-extension://"))
}

fn extension_response(headers: &HeaderMap, mut response: Response) -> Response {
    let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    else {
        return response;
    };
    if !origin.starts_with("chrome-extension://") {
        return response;
    }
    let response_headers = response.headers_mut();
    if let Ok(origin) = HeaderValue::from_str(origin) {
        response_headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
    }
    response_headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, OPTIONS"),
    );
    response_headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("Content-Type"),
    );
    response_headers.insert(header::VARY, HeaderValue::from_static("Origin"));
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_url_must_match_domain_and_path() {
        let context = ContextAction {
            title: "Download".into(),
            paths: vec!["/explore/*".into()],
            active_tab: Some(opencli_rs_core::ActiveTabAction {
                use_pipeline: true,
                extract: None,
            }),
            args: HashMap::new(),
        };
        assert!(context_matches(
            Some("www.xiaohongshu.com"),
            &context,
            &Url::parse("https://www.xiaohongshu.com/explore/note").unwrap()
        ));
        assert!(!context_matches(
            Some("www.xiaohongshu.com"),
            &context,
            &Url::parse("https://www.xiaohongshu.com/search_result").unwrap()
        ));
        assert!(!context_matches(
            Some("www.xiaohongshu.com"),
            &context,
            &Url::parse("https://example.com/explore/note").unwrap()
        ));
    }
}
