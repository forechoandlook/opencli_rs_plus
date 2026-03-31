use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct PipelineContext {
    pub data: Value,
    pub args: HashMap<String, Value>,
}

impl PipelineContext {
    pub fn new(args: HashMap<String, Value>) -> Self {
        Self {
            data: Value::Null,
            args,
        }
    }
}
