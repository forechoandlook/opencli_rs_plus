use opencli_rs_browser::{get_app_port, probe_cdp, BrowserBridge, CdpPage, DaemonClient};
use opencli_rs_core::{CliCommand, CliError, IPage, Strategy};
use opencli_rs_pipeline::{execute_pipeline, steps::register_all_steps, StepRegistry};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// Get daemon port from env or fall back to the stable default 19825.
fn daemon_port() -> u16 {
    std::env::var("OPENCLI_DAEMON_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(19825)
}

/// Get command timeout from env or command config or default (60s)
fn command_timeout(cmd: &CliCommand) -> u64 {
    std::env::var("OPENCLI_BROWSER_COMMAND_TIMEOUT")
        .ok()
        .and_then(|s| s.parse().ok())
        .or(cmd.timeout_seconds)
        .unwrap_or(60)
}

/// Keep browser pages by their configured origin rather than command name.
/// Different commands for the same site therefore reuse one authenticated,
/// minimized page, while the task panel still records the exact command.
fn browser_workspace(domain: Option<&str>, site: &str) -> String {
    let domain = domain.unwrap_or(site);
    format!(
        "origin:{}",
        domain.trim().trim_end_matches('/').to_ascii_lowercase()
    )
}

fn task_workspace(site: &str, name: &str) -> String {
    format!("adapter:{site}:{name}")
}

pub async fn execute_command(
    cmd: &CliCommand,
    kwargs: HashMap<String, Value>,
) -> Result<Value, CliError> {
    let timeout_secs = command_timeout(cmd);

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        execute_command_inner(cmd, kwargs),
    )
    .await;

    match result {
        Ok(inner) => inner,
        Err(_) => Err(CliError::timeout(format!(
            "Command '{}' timed out after {}s",
            cmd.full_name(),
            timeout_secs
        ))),
    }
}

async fn execute_command_inner(
    cmd: &CliCommand,
    kwargs: HashMap<String, Value>,
) -> Result<Value, CliError> {
    // Build step registry
    let mut registry = StepRegistry::new();
    register_all_steps(&mut registry);

    if cmd.needs_browser() {
        // UI strategy + localhost domain → try direct CDP connection to Electron app
        let is_electron = cmd.strategy == Strategy::Ui
            && cmd
                .domain
                .as_deref()
                .is_some_and(|d| d == "localhost" || d.starts_with("localhost:"));

        let mut popup_task = None;
        let page: Arc<dyn IPage> = if is_electron {
            let port = get_app_port(&cmd.site).ok_or_else(|| {
                CliError::browser_connect(format!(
                    "No Electron app registered for '{}'. Add it to ~/.opencli-rs/apps.yaml",
                    cmd.site
                ))
            })?;
            let ws_url = probe_cdp(port).await?;
            tracing::debug!(site = %cmd.site, port = port, "Connecting via CDP to Electron app");
            Arc::new(CdpPage::connect(&ws_url).await?)
        } else {
            // Standard browser session via Chrome extension
            let mut bridge = BrowserBridge::new(daemon_port());
            // Page workspaces are keyed by origin. This reuses the cached,
            // authenticated page across commands for the same website without
            // ever targeting a user-owned tab.
            let page_workspace = browser_workspace(cmd.domain.as_deref(), &cmd.site);
            let page = bridge.connect_with_workspace(&page_workspace).await?;
            // The task panel is supplementary observability. Do not turn a
            // reporting failure into an adapter failure.
            let task_workspace = task_workspace(&cmd.site, &cmd.name);
            popup_task = DaemonClient::new(daemon_port())
                .start_task(&task_workspace)
                .await
                .ok()
                .map(|task| (DaemonClient::new(daemon_port()), task.id));
            page
        };

        // Pre-navigate only for Cookie/Header strategies.
        let pipeline_starts_with_navigate = cmd
            .pipeline
            .as_ref()
            .and_then(|steps| steps.first())
            .and_then(|step| step.as_object())
            .is_some_and(|obj| obj.contains_key("navigate"));

        let should_pre_navigate = matches!(cmd.strategy, Strategy::Cookie | Strategy::Header);
        if should_pre_navigate && !pipeline_starts_with_navigate {
            if let Some(domain) = &cmd.domain {
                let already_on_origin = page
                    .url()
                    .await
                    .ok()
                    .and_then(|current| {
                        let host = current
                            .split("://")
                            .nth(1)
                            .unwrap_or(&current)
                            .split('/')
                            .next()
                            .unwrap_or("");
                        Some(host.eq_ignore_ascii_case(domain.trim()))
                    })
                    .unwrap_or(false);
                if already_on_origin {
                    tracing::debug!(domain = %domain, "Skip pre-navigate; page already on origin");
                } else {
                    let url = format!("https://{}", domain);
                    tracing::debug!(url = %url, "Pre-navigating to domain");
                    page.goto(&url, None).await?;
                }
            }
        }

        opencli_rs_pipeline::helpers::set_helper_root(cmd.source_dir.clone());

        // Execute
        let result = if let Some(ref steps) = cmd.pipeline {
            execute_pipeline(Some(page.clone()), steps, &kwargs, &registry).await
        } else if cmd.func.is_some() {
            run_command(cmd, Some(page.clone()), &kwargs, &registry).await
        } else {
            Err(CliError::command_execution(format!(
                "Command '{}' has no pipeline or func",
                cmd.full_name()
            )))
        };

        if let Some((client, task_id)) = popup_task {
            match &result {
                Ok(data) => {
                    let _ = client.finish_task(&task_id, "done", Some(data), None).await;
                }
                Err(err) => {
                    let _ = client
                        .finish_task(&task_id, "failed", None, Some(&err.to_string()))
                        .await;
                }
            }
        }

        // Extension-backed pages are left for the extension's short idle
        // cleanup window. This makes subsequent commands reuse a background
        // tab and its warm cache, without touching the user's visible tab.
        // Electron/CDP targets retain their existing close behaviour.
        if is_electron {
            let _ = page.close().await;
        }

        opencli_rs_pipeline::helpers::set_helper_root(None);
        result
    } else {
        opencli_rs_pipeline::helpers::set_helper_root(cmd.source_dir.clone());
        let result = run_command(cmd, None, &kwargs, &registry).await;
        opencli_rs_pipeline::helpers::set_helper_root(None);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_workspace_groups_commands_by_configured_origin() {
        assert_eq!(
            browser_workspace(Some("www.zhihu.com"), "zhihu"),
            browser_workspace(Some("www.zhihu.com"), "zhihu")
        );
        assert_eq!(
            browser_workspace(Some("www.zhihu.com/"), "zhihu"),
            "origin:www.zhihu.com"
        );
        assert_ne!(
            task_workspace("zhihu", "hot"),
            task_workspace("zhihu", "search")
        );
    }
}

async fn run_command(
    cmd: &CliCommand,
    page: Option<Arc<dyn IPage>>,
    kwargs: &HashMap<String, Value>,
    registry: &StepRegistry,
) -> Result<Value, CliError> {
    if let Some(pipeline) = &cmd.pipeline {
        execute_pipeline(page, pipeline, kwargs, registry).await
    } else if let Some(func) = &cmd.func {
        func(page, kwargs.clone()).await
    } else {
        Err(CliError::command_execution(format!(
            "Command '{}' has no pipeline or func",
            cmd.full_name()
        )))
    }
}
