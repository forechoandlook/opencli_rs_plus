use serde_json::Value;
use serde_json::Map;

use crate::csv_out::render_csv;
use crate::format::{OutputFormat, RenderOptions};
use crate::json::render_json;
use crate::markdown::render_markdown;
use crate::table::render_table;
use crate::yaml::render_yaml;

fn resolve_fields<'a>(opts: &'a RenderOptions) -> Option<&'a [String]> {
    opts.fields.as_deref().or(opts.columns.as_deref())
}

fn project_fields(data: &Value, fields: Option<&[String]>) -> Value {
    let Some(fields) = fields else {
        return data.clone();
    };
    if fields.is_empty() {
        return data.clone();
    }
    match data {
        Value::Array(arr) => Value::Array(
            arr.iter()
                .map(|item| project_fields(item, Some(fields)))
                .collect(),
        ),
        Value::Object(obj) => {
            let mut out = Map::new();
            for field in fields {
                if let Some(v) = obj.get(field) {
                    out.insert(field.clone(), v.clone());
                }
            }
            Value::Object(out)
        }
        _ => data.clone(),
    }
}

/// Build a footer string from render options.
fn build_footer(opts: &RenderOptions) -> Option<String> {
    let mut parts = Vec::new();

    if let Some(elapsed) = &opts.elapsed {
        let secs = elapsed.as_secs_f64();
        if secs < 1.0 {
            parts.push(format!("Elapsed: {:.0}ms", secs * 1000.0));
        } else {
            parts.push(format!("Elapsed: {:.2}s", secs));
        }
    }

    if let Some(source) = &opts.source {
        parts.push(format!("Source: {}", source));
    }

    if let Some(extra) = &opts.footer_extra {
        parts.push(extra.clone());
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" | "))
    }
}

/// Render data according to the given options, returning the formatted string.
pub fn render(data: &Value, opts: &RenderOptions) -> String {
    let fields = resolve_fields(opts);
    let data = project_fields(data, fields);
    let cols = fields;

    let mut output = match opts.format {
        OutputFormat::Table => render_table(&data, cols),
        OutputFormat::Json => render_json(&data, cols),
        OutputFormat::Yaml => render_yaml(&data, cols),
        OutputFormat::Csv => render_csv(&data, cols),
        OutputFormat::Markdown => render_markdown(&data, cols),
    };

    if let Some(title) = &opts.title {
        output = format!("{}\n{}", title, output);
    }

    if let Some(footer) = build_footer(opts) {
        if !output.ends_with('\n') {
            output.push('\n');
        }
        output.push_str(&footer);
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{OutputFormat, RenderOptions};
    use serde_json::json;
    use std::time::Duration;

    #[test]
    fn test_render_table_format() {
        let data = json!([{"name": "Alice", "age": 30}]);
        let opts = RenderOptions {
            format: OutputFormat::Table,
            ..Default::default()
        };
        let out = render(&data, &opts);
        assert!(out.contains("Alice"));
    }

    #[test]
    fn test_render_json_format() {
        let data = json!({"name": "Alice"});
        let opts = RenderOptions {
            format: OutputFormat::Json,
            ..Default::default()
        };
        let out = render(&data, &opts);
        assert!(out.contains("Alice"));
    }

    #[test]
    fn test_render_with_footer() {
        let data = json!({"name": "Alice"});
        let opts = RenderOptions {
            format: OutputFormat::Json,
            elapsed: Some(Duration::from_millis(150)),
            source: Some("test-api".to_string()),
            footer_extra: Some("page 1/3".to_string()),
            ..Default::default()
        };
        let out = render(&data, &opts);
        assert!(out.contains("Elapsed: 150ms"));
        assert!(out.contains("Source: test-api"));
        assert!(out.contains("page 1/3"));
    }

    #[test]
    fn test_render_with_title() {
        let data = json!([{"name": "Alice"}]);
        let opts = RenderOptions {
            format: OutputFormat::Table,
            title: Some("User List".to_string()),
            ..Default::default()
        };
        let out = render(&data, &opts);
        assert!(out.starts_with("User List\n"));
    }

    #[test]
    fn test_render_elapsed_seconds() {
        let data = json!("ok");
        let opts = RenderOptions {
            format: OutputFormat::Json,
            elapsed: Some(Duration::from_secs_f64(2.5)),
            ..Default::default()
        };
        let out = render(&data, &opts);
        assert!(out.contains("Elapsed: 2.50s"));
    }
}
