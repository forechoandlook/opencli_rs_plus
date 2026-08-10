//! Pipeline steps for the local opencli KV store (`~/.opencli-rs/kv.json`).
//!
//! - `kv_get`: read a key into pipeline data (or a field on an object)
//! - `kv_set`: write a key from params/data; does not change pipeline data on success

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use opencli_rs_core::{kv, CliError, IPage};
use serde_json::{json, Value};

use crate::step_registry::{StepHandler, StepRegistry};
use crate::template::{render_template_str, TemplateContext};

fn ctx(data: &Value, args: &HashMap<String, Value>) -> TemplateContext {
    TemplateContext {
        args: args.clone(),
        data: data.clone(),
        item: Value::Null,
        index: 0,
    }
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

fn render_param(
    params: &Value,
    key: &str,
    data: &Value,
    args: &HashMap<String, Value>,
) -> Result<Option<String>, CliError> {
    let Some(raw) = params.get(key) else {
        return Ok(None);
    };
    match raw {
        Value::String(s) => {
            let rendered = render_template_str(s, &ctx(data, args))?;
            Ok(Some(value_to_string(&rendered)))
        }
        other => Ok(Some(value_to_string(other))),
    }
}

fn render_value(
    params: &Value,
    key: &str,
    data: &Value,
    args: &HashMap<String, Value>,
) -> Result<Option<Value>, CliError> {
    let Some(raw) = params.get(key) else {
        return Ok(None);
    };
    match raw {
        Value::String(s) => Ok(Some(render_template_str(s, &ctx(data, args))?)),
        other => Ok(Some(other.clone())),
    }
}

fn is_empty_value(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::String(s) => s.trim().is_empty(),
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// kv_get
// ---------------------------------------------------------------------------

/// Read a key from the local KV store.
///
/// Params (object):
/// - `key` (required): template string
/// - `field` (optional): if set, merge value into current data object at this field
/// - `only_if_empty` (optional bool): with `field`, only write when field missing/empty
/// - `default` (optional): value used when key is missing
/// - `required` (optional bool): error when missing and no default
///
/// String form: `kv_get: "site:me.userId"` is sugar for `{ key: "..." }`.
pub struct KvGetStep;

#[async_trait]
impl StepHandler for KvGetStep {
    fn name(&self) -> &'static str {
        "kv_get"
    }

    async fn execute(
        &self,
        _page: Option<Arc<dyn IPage>>,
        params: &Value,
        data: &Value,
        args: &HashMap<String, Value>,
    ) -> Result<Value, CliError> {
        let params = match params {
            Value::String(key) => json!({ "key": key }),
            Value::Object(_) => params.clone(),
            _ => {
                return Err(CliError::pipeline(
                    "kv_get: params must be a key string or object",
                ))
            }
        };

        let key = render_param(&params, "key", data, args)?
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| CliError::pipeline("kv_get: missing key"))?;

        let only_if_empty = params
            .get("only_if_empty")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let required = params
            .get("required")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let field = params
            .get("field")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);

        if let Some(ref field_name) = field {
            if only_if_empty {
                if let Some(existing) = data.get(field_name) {
                    if !is_empty_value(existing) {
                        return Ok(data.clone());
                    }
                }
            }
        }

        let stored = kv::get(&key).map_err(CliError::pipeline)?;
        let value = match stored {
            Some(v) => v,
            None => {
                if let Some(default) = render_value(&params, "default", data, args)? {
                    default
                } else if required {
                    return Err(CliError::pipeline(format!("kv_get: key '{key}' not found")));
                } else {
                    Value::Null
                }
            }
        };

        if let Some(field_name) = field {
            let mut obj = match data {
                Value::Object(map) => map.clone(),
                Value::Null => serde_json::Map::new(),
                other => {
                    let mut map = serde_json::Map::new();
                    map.insert("_data".into(), other.clone());
                    map
                }
            };
            if !(only_if_empty
                && obj
                    .get(&field_name)
                    .is_some_and(|v| !is_empty_value(v)))
            {
                obj.insert(field_name, value);
            }
            Ok(Value::Object(obj))
        } else {
            Ok(value)
        }
    }
}

// ---------------------------------------------------------------------------
// kv_set
// ---------------------------------------------------------------------------

/// Write a key. Pipeline `data` is returned unchanged.
///
/// Params:
/// - `key` (required)
/// - `value` (optional): template/JSON; defaults to current `data`
/// - `ttl` (optional): e.g. `30d`, `24h`, `15m`, `60s`, or bare seconds
/// - `skip_empty` (optional bool, default true): skip write when value is null/empty
pub struct KvSetStep;

#[async_trait]
impl StepHandler for KvSetStep {
    fn name(&self) -> &'static str {
        "kv_set"
    }

    async fn execute(
        &self,
        _page: Option<Arc<dyn IPage>>,
        params: &Value,
        data: &Value,
        args: &HashMap<String, Value>,
    ) -> Result<Value, CliError> {
        let params = match params {
            Value::Object(_) => params.clone(),
            _ => {
                return Err(CliError::pipeline(
                    "kv_set: params must be an object with key/value",
                ))
            }
        };

        let key = render_param(&params, "key", data, args)?
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| CliError::pipeline("kv_set: missing key"))?;

        let value = match render_value(&params, "value", data, args)? {
            Some(v) => v,
            None => data.clone(),
        };

        let skip_empty = params
            .get("skip_empty")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        if skip_empty && is_empty_value(&value) {
            return Ok(data.clone());
        }

        let ttl = render_param(&params, "ttl", data, args)?;
        let ttl_ref = ttl.as_deref().filter(|s| !s.trim().is_empty());

        kv::set(&key, value, ttl_ref).map_err(CliError::pipeline)?;
        Ok(data.clone())
    }
}

pub fn register_kv_steps(registry: &mut StepRegistry) {
    registry.register(Arc::new(KvGetStep));
    registry.register(Arc::new(KvSetStep));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[tokio::test]
    async fn kv_get_merges_field_only_if_empty() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("opencli-kv-step-{nanos}.json"));
        let _ = std::fs::remove_file(&path);
        // Point home store away by writing via API path helpers is hard; use public API
        // which writes to real kv path — isolate with unique keys instead.
        let key = format!("test:kv_get:{nanos}");
        kv::set(&key, json!("from-kv"), None).unwrap();

        let step = KvGetStep;
        let data = json!({ "userId": "already" });
        let out = step
            .execute(
                None,
                &json!({ "key": key, "field": "userId", "only_if_empty": true }),
                &data,
                &HashMap::new(),
            )
            .await
            .unwrap();
        assert_eq!(out["userId"], json!("already"));

        let data2 = json!({ "userId": "" });
        let out2 = step
            .execute(
                None,
                &json!({ "key": key, "field": "userId", "only_if_empty": true }),
                &data2,
                &HashMap::new(),
            )
            .await
            .unwrap();
        assert_eq!(out2["userId"], json!("from-kv"));

        let _ = kv::del(&key);
        let _ = std::fs::remove_file(&path);
    }
}
