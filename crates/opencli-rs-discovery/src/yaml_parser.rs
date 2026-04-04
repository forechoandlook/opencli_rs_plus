use opencli_rs_core::{ArgDef, ArgType, CliCommand, CliError, NavigateBefore, Strategy};
use serde_json::Value;

/// Parse a YAML adapter file content into a CliCommand.
pub fn parse_yaml_adapter(content: &str) -> Result<CliCommand, CliError> {
    let raw: Value = serde_yaml::from_str(content).map_err(|e| CliError::AdapterLoad {
        message: format!("Failed to parse YAML: {}", e),
        suggestions: vec![],
        source: None,
    })?;

    let site = raw
        .get("site")
        .and_then(|v| v.as_str())
        .ok_or_else(|| CliError::AdapterLoad {
            message: "Missing 'site' field".into(),
            suggestions: vec![],
            source: None,
        })?
        .to_string();

    let name = raw
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| CliError::AdapterLoad {
            message: "Missing 'name' field".into(),
            suggestions: vec![],
            source: None,
        })?
        .to_string();

    // Parse strategy (default: public)
    let strategy = match raw.get("strategy").and_then(|v| v.as_str()) {
        Some(s) => serde_json::from_value(Value::String(s.to_string())).unwrap_or(Strategy::Public),
        None => Strategy::Public,
    };

    // Parse args — in YAML they're a map: { limit: { type: int, default: 20 } }
    let args = parse_args(&raw)?;

    // Parse columns
    let columns = raw
        .get("columns")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    // Pipeline is stored as-is (Vec<Value>)
    let pipeline = raw.get("pipeline").and_then(|v| v.as_array()).cloned();

    Ok(CliCommand {
        site,
        name,
        description: raw
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        domain: raw.get("domain").and_then(|v| v.as_str()).map(String::from),
        strategy,
        browser: raw
            .get("browser")
            .and_then(|v| v.as_bool())
            .unwrap_or(strategy.requires_browser()),
        args,
        columns,
        pipeline,
        func: None,
        timeout_seconds: raw.get("timeoutSeconds").and_then(|v| v.as_u64()),
        navigate_before: NavigateBefore::default(),
        version: raw
            .get("version")
            .and_then(|v| v.as_str())
            .map(String::from),
        updated_at: raw
            .get("updatedAt")
            .and_then(|v| v.as_str())
            .map(String::from),
    })
}

/// Parse args from YAML map format to Vec<ArgDef>
fn parse_args(raw: &Value) -> Result<Vec<ArgDef>, CliError> {
    let args_val = match raw.get("args") {
        Some(v) if v.is_object() => v,
        Some(v) if v.is_array() && v.as_array().unwrap().is_empty() => return Ok(vec![]),
        _ => return Ok(vec![]),
    };

    let map = args_val.as_object().unwrap();
    let mut result = vec![];

    for (name, def) in map {
        let arg_type = match def.get("type").and_then(|v| v.as_str()) {
            Some("int") => ArgType::Int,
            Some("number") => ArgType::Number,
            Some("bool") => ArgType::Bool,
            Some("boolean") => ArgType::Boolean,
            _ => ArgType::Str,
        };

        result.push(ArgDef {
            name: name.clone(),
            arg_type,
            required: def
                .get("required")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            positional: def
                .get("positional")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            description: def
                .get("description")
                .and_then(|v| v.as_str())
                .map(String::from),
            choices: def.get("choices").and_then(|v| v.as_array()).map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            }),
            default: def.get("default").cloned(),
        });
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_adapter() {
        let yaml = r#"
site: hackernews
name: top
description: Top stories
strategy: public
browser: false
args:
  limit:
    type: int
    default: 20
    description: Number of items
columns: [rank, title, score, author]
pipeline:
  - fetch: https://hacker-news.firebaseio.com/v0/topstories.json
  - limit: "${{ args.limit }}"
"#;
        let cmd = parse_yaml_adapter(yaml).unwrap();
        assert_eq!(cmd.site, "hackernews");
        assert_eq!(cmd.name, "top");
        assert_eq!(cmd.strategy, Strategy::Public);
        assert!(!cmd.browser);
        assert_eq!(cmd.args.len(), 1);
        assert_eq!(cmd.args[0].name, "limit");
        assert_eq!(cmd.args[0].arg_type, ArgType::Int);
        assert_eq!(cmd.columns, vec!["rank", "title", "score", "author"]);
        assert!(cmd.pipeline.is_some());
        assert_eq!(cmd.pipeline.unwrap().len(), 2);
    }

    #[test]
    fn test_parse_cookie_strategy() {
        let yaml = r#"
site: bilibili
name: hot
description: Hot videos
strategy: cookie
domain: www.bilibili.com
"#;
        let cmd = parse_yaml_adapter(yaml).unwrap();
        assert_eq!(cmd.strategy, Strategy::Cookie);
        assert!(cmd.browser); // cookie strategy implies browser
        assert_eq!(cmd.domain, Some("www.bilibili.com".to_string()));
    }

    #[test]
    fn test_parse_missing_site_errors() {
        let yaml = "name: test\n";
        assert!(parse_yaml_adapter(yaml).is_err());
    }

    #[test]
    fn test_parse_missing_name_errors() {
        let yaml = "site: test\n";
        assert!(parse_yaml_adapter(yaml).is_err());
    }

    #[test]
    fn test_invalid_yaml_errors() {
        let yaml = "site: [unclosed bracket\nname: test";
        assert!(parse_yaml_adapter(yaml).is_err());
    }

    #[test]
    fn test_unknown_arg_type_defaults_to_str() {
        let yaml = r#"
site: test
name: cmd
args:
  ts:
    type: datetime
    description: A timestamp
"#;
        let cmd = parse_yaml_adapter(yaml).unwrap();
        assert_eq!(cmd.args[0].arg_type, ArgType::Str);
    }

    #[test]
    fn test_arg_types_all_variants() {
        let yaml = r#"
site: test
name: cmd
args:
  a:
    type: int
  b:
    type: number
  c:
    type: bool
  d:
    type: boolean
  e:
    type: str
"#;
        let cmd = parse_yaml_adapter(yaml).unwrap();
        // HashMap — 顺序不固定，按名字查
        let find = |name: &str| {
            cmd.args
                .iter()
                .find(|a| a.name == name)
                .unwrap()
                .arg_type
                .clone()
        };
        assert_eq!(find("a"), ArgType::Int);
        assert_eq!(find("b"), ArgType::Number);
        assert_eq!(find("c"), ArgType::Bool);
        assert_eq!(find("d"), ArgType::Boolean);
        assert_eq!(find("e"), ArgType::Str);
    }

    #[test]
    fn test_empty_args_object() {
        let yaml = "site: test\nname: cmd\nargs: {}\n";
        let cmd = parse_yaml_adapter(yaml).unwrap();
        assert!(cmd.args.is_empty());
    }

    #[test]
    fn test_no_args_field() {
        let yaml = "site: test\nname: cmd\n";
        let cmd = parse_yaml_adapter(yaml).unwrap();
        assert!(cmd.args.is_empty());
    }

    #[test]
    fn test_no_pipeline_field() {
        let yaml = "site: test\nname: cmd\n";
        let cmd = parse_yaml_adapter(yaml).unwrap();
        assert!(cmd.pipeline.is_none());
    }

    #[test]
    fn test_empty_pipeline_array() {
        // 空 pipeline 数组（不常见但不应 crash）
        let yaml = "site: test\nname: cmd\npipeline: []\n";
        let cmd = parse_yaml_adapter(yaml).unwrap();
        // 空数组在 as_array() 返回 Some([])，pipeline 应该是 Some([])
        assert!(cmd.pipeline.is_some());
        assert_eq!(cmd.pipeline.unwrap().len(), 0);
    }

    #[test]
    fn test_required_arg() {
        let yaml = r#"
site: test
name: cmd
args:
  query:
    required: true
    description: Search keyword
"#;
        let cmd = parse_yaml_adapter(yaml).unwrap();
        let arg = &cmd.args[0];
        assert!(arg.required);
        assert_eq!(arg.name, "query");
    }

    #[test]
    fn test_positional_arg() {
        let yaml = r#"
site: test
name: cmd
args:
  query:
    positional: true
    required: true
    description: Keyword
"#;
        let cmd = parse_yaml_adapter(yaml).unwrap();
        let arg = &cmd.args[0];
        assert!(arg.positional);
        assert!(arg.required);
    }

    #[test]
    fn test_arg_with_choices() {
        let yaml = r#"
site: test
name: cmd
args:
  format:
    type: str
    default: json
    choices: [json, csv, table]
    description: Output format
"#;
        let cmd = parse_yaml_adapter(yaml).unwrap();
        let arg = &cmd.args[0];
        let choices = arg.choices.as_ref().expect("choices should be present");
        assert_eq!(choices.len(), 3);
        assert!(choices.contains(&"json".to_string()));
    }

    #[test]
    fn test_arg_default_value() {
        let yaml = r#"
site: test
name: cmd
args:
  limit:
    type: int
    default: 20
"#;
        let cmd = parse_yaml_adapter(yaml).unwrap();
        let arg = &cmd.args[0];
        assert_eq!(arg.default, Some(serde_json::json!(20)));
    }

    #[test]
    fn test_timeout_seconds() {
        let yaml = "site: test\nname: cmd\ntimeoutSeconds: 30\n";
        let cmd = parse_yaml_adapter(yaml).unwrap();
        assert_eq!(cmd.timeout_seconds, Some(30));
    }

    #[test]
    fn test_version_and_updated_at() {
        let yaml = r#"
site: test
name: cmd
version: "1.2.3"
updatedAt: "2026-01-01"
"#;
        let cmd = parse_yaml_adapter(yaml).unwrap();
        assert_eq!(cmd.version.as_deref(), Some("1.2.3"));
        assert_eq!(cmd.updated_at.as_deref(), Some("2026-01-01"));
    }

    #[test]
    fn test_description_defaults_to_empty_string() {
        let yaml = "site: test\nname: cmd\n";
        let cmd = parse_yaml_adapter(yaml).unwrap();
        assert_eq!(cmd.description, "");
    }

    #[test]
    fn test_strategy_defaults_to_public() {
        let yaml = "site: test\nname: cmd\n";
        let cmd = parse_yaml_adapter(yaml).unwrap();
        assert_eq!(cmd.strategy, Strategy::Public);
    }

    #[test]
    fn test_public_strategy_browser_defaults_to_false() {
        let yaml = "site: test\nname: cmd\nstrategy: public\n";
        let cmd = parse_yaml_adapter(yaml).unwrap();
        assert!(!cmd.browser);
    }

    #[test]
    fn test_cookie_strategy_browser_defaults_to_true() {
        let yaml = "site: test\nname: cmd\nstrategy: cookie\n";
        let cmd = parse_yaml_adapter(yaml).unwrap();
        // cookie strategy.requires_browser() == true
        assert!(cmd.browser);
    }

    #[test]
    fn test_explicit_browser_false_overrides_strategy() {
        // 这是一个记录当前行为的测试：显式设置 browser: false 会覆盖 strategy 的默认值。
        // 这个行为可能是预期的（某些 cookie adapter 用 api 模式），也可能是 bug。
        // 先记录下来。
        let yaml = "site: test\nname: cmd\nstrategy: cookie\nbrowser: false\n";
        let cmd = parse_yaml_adapter(yaml).unwrap();
        // 当前行为：explicit browser: false 优先于 strategy
        assert!(
            !cmd.browser,
            "explicit browser: false should override strategy default"
        );
    }

    #[test]
    fn test_columns_parsed_correctly() {
        let yaml = r#"
site: test
name: cmd
columns: [rank, title, author, score]
"#;
        let cmd = parse_yaml_adapter(yaml).unwrap();
        assert_eq!(cmd.columns, vec!["rank", "title", "author", "score"]);
    }

    #[test]
    fn test_no_columns_defaults_to_empty() {
        let yaml = "site: test\nname: cmd\n";
        let cmd = parse_yaml_adapter(yaml).unwrap();
        assert!(cmd.columns.is_empty());
    }

    #[test]
    fn test_pipeline_steps_count() {
        let yaml = r#"
site: test
name: cmd
pipeline:
  - fetch: https://example.com
  - map:
      title: ${{ item.title }}
  - limit: 10
"#;
        let cmd = parse_yaml_adapter(yaml).unwrap();
        let pipeline = cmd.pipeline.unwrap();
        assert_eq!(pipeline.len(), 3);
    }
}
