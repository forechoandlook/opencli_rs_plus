use opencli_rs_browser::{get_app_port, probe_cdp, BrowserBridge, CdpPage};
use opencli_rs_core::{CliCommand, CliError, IPage, Strategy};
use opencli_rs_pipeline::{execute_pipeline, steps::register_all_steps, StepRegistry};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// Get daemon port from env when explicitly pinned.
fn daemon_port() -> Option<u16> {
    std::env::var("OPENCLI_DAEMON_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
}

/// Get command timeout from env or command config or default (60s)
fn command_timeout(cmd: &CliCommand) -> u64 {
    std::env::var("OPENCLI_BROWSER_COMMAND_TIMEOUT")
        .ok()
        .and_then(|s| s.parse().ok())
        .or(cmd.timeout_seconds)
        .unwrap_or(60)
}

pub async fn execute_command(
    cmd: &CliCommand,
    kwargs: HashMap<String, Value>,
) -> Result<Value, CliError> {
    tracing::info!(site = %cmd.site, name = %cmd.name, "Executing command");

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
                .map_or(false, |d| d == "localhost" || d.starts_with("localhost:"));

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
            let mut bridge = match daemon_port() {
                Some(port) => BrowserBridge::new(port),
                None => BrowserBridge::default_port(),
            };
            bridge.connect().await?
        };

        // Pre-navigate only for Cookie/Header strategies.
        let pipeline_starts_with_navigate = cmd
            .pipeline
            .as_ref()
            .and_then(|steps| steps.first())
            .and_then(|step| step.as_object())
            .map_or(false, |obj| obj.contains_key("navigate"));

        let should_pre_navigate = matches!(cmd.strategy, Strategy::Cookie | Strategy::Header);
        if should_pre_navigate && !pipeline_starts_with_navigate {
            if let Some(domain) = &cmd.domain {
                let url = format!("https://{}", domain);
                tracing::debug!(url = %url, "Pre-navigating to domain");
                page.goto(&url, None).await?;
            }
        }

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

        // Close the automation tab/window after command completes
        let _ = page.close().await;

        result
    } else {
        run_command(cmd, None, &kwargs, &registry).await
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
