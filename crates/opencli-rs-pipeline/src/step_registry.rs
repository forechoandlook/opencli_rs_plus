use async_trait::async_trait;
use opencli_rs_core::{CliError, IPage};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

#[async_trait]
pub trait StepHandler: Send + Sync {
    /// The name used to identify this step in YAML pipelines.
    fn name(&self) -> &'static str;

    /// Execute this step.
    /// - `page`: optional browser page (None for non-browser commands)
    /// - `params`: the step's parameters (from YAML)
    /// - `data`: current pipeline data state
    /// - `args`: command-line arguments
    async fn execute(
        &self,
        page: Option<Arc<dyn IPage>>,
        params: &Value,
        data: &Value,
        args: &HashMap<String, Value>,
    ) -> Result<Value, CliError>;

    /// Whether this step is a browser step (eligible for retry on transient errors).
    fn is_browser_step(&self) -> bool {
        false
    }
}

pub struct StepRegistry {
    handlers: HashMap<String, Arc<dyn StepHandler>>,
}

impl StepRegistry {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    pub fn register(&mut self, handler: Arc<dyn StepHandler>) {
        self.handlers.insert(handler.name().to_string(), handler);
    }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn StepHandler>> {
        self.handlers.get(name)
    }
}

impl Default for StepRegistry {
    fn default() -> Self {
        Self::new()
    }
}
