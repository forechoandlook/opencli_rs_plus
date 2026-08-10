use crate::{ArgDef, CliError, IPage, Strategy};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

pub type CommandArgs = HashMap<String, Value>;

pub type AdapterFunc = Arc<
    dyn Fn(
            Option<Arc<dyn IPage>>,
            CommandArgs,
        ) -> Pin<Box<dyn Future<Output = Result<Value, CliError>> + Send>>
        + Send
        + Sync,
>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NavigateBefore {
    Bool(bool),
    Url(String),
}

impl Default for NavigateBefore {
    fn default() -> Self {
        Self::Bool(true)
    }
}

/// Declares that an adapter can be offered from the browser extension for a
/// matching user-owned page. The extension interprets the explicitly declared
/// active-tab plan in that current tab; the normal engine is not involved.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActiveTabAction {
    /// Reuse the adapter's existing YAML pipeline, while skipping mutating
    /// browser steps such as navigate in the extension runtime.
    #[serde(default, rename = "usePipeline")]
    pub use_pipeline: bool,
    /// Optional read-only JavaScript extractor for adapters whose normal
    /// pipeline requires network/tap steps and cannot run from current state.
    #[serde(default)]
    pub extract: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContextAction {
    /// Short label shown in the extension popup.
    pub title: String,
    /// Optional exact hostnames for this action. When omitted, adapter `domain`
    /// remains the matching boundary.
    #[serde(default)]
    pub hosts: Vec<String>,
    /// URL paths this action accepts. `*` matches any sequence of characters.
    #[serde(default)]
    pub paths: Vec<String>,
    /// Declarative current-tab execution plan. Its presence opts an adapter
    /// into direct current-page extraction instead of background execution.
    #[serde(default, rename = "activeTab")]
    pub active_tab: Option<ActiveTabAction>,
    /// Adapter argument -> approved contextual value. Currently only
    /// `current_url` is accepted and passed to YAML template rendering.
    #[serde(default)]
    pub args: HashMap<String, String>,
}

#[derive(Clone)]
pub struct CliCommand {
    pub site: String,
    pub name: String,
    pub description: String,
    pub domain: Option<String>,
    pub strategy: Strategy,
    pub browser: bool,
    pub args: Vec<ArgDef>,
    pub columns: Vec<String>,
    pub pipeline: Option<Vec<Value>>,
    pub func: Option<AdapterFunc>,
    pub timeout_seconds: Option<u64>,
    pub navigate_before: NavigateBefore,
    /// Adapter version, e.g. "1.0.0"
    pub version: Option<String>,
    /// Last updated timestamp (ISO 8601)
    pub updated_at: Option<String>,
    /// Optional browser-popup declaration parsed from the adapter YAML.
    pub context: Option<ContextAction>,
}

impl CliCommand {
    pub fn full_name(&self) -> String {
        format!("{} {}", self.site, self.name)
    }

    pub fn needs_browser(&self) -> bool {
        if self.browser || self.strategy.requires_browser() {
            return true;
        }
        // Check if pipeline contains browser steps
        if let Some(ref pipeline) = self.pipeline {
            const BROWSER_STEPS: &[&str] = &[
                "navigate",
                "click",
                "type",
                "wait",
                "press",
                "evaluate",
                "snapshot",
                "screenshot",
                "intercept",
                "tap",
                "bg_fetch",
            ];
            for step in pipeline {
                if let Some(obj) = step.as_object() {
                    for key in obj.keys() {
                        if BROWSER_STEPS.contains(&key.as_str()) {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }
}

impl std::fmt::Debug for CliCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CliCommand")
            .field("site", &self.site)
            .field("name", &self.name)
            .field("strategy", &self.strategy)
            .field("browser", &self.browser)
            .field("has_func", &self.func.is_some())
            .field("has_pipeline", &self.pipeline.is_some())
            .finish()
    }
}
