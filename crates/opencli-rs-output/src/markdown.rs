use serde_json::Value;

fn resolve_columns(data: &Value, columns: Option<&[String]>) -> Vec<String> {
    if let Some(cols) = columns {
        return cols.to_vec();
    }
    match data {
        Value::Array(arr) => {
            if let Some(Value::Object(obj)) = arr.first() {
                obj.keys().cloned().collect()
            } else {
                vec![]
            }
        }
        Value::Object(obj) => obj.keys().cloned().collect(),
        _ => vec![],
    }
}

fn value_to_cell(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

fn escape_pipe(s: &str) -> String {
    s.replace('|', "\\|")
}

/// Render data as a Markdown table.
pub fn render_markdown(data: &Value, columns: Option<&[String]>) -> String {
    match data {
        Value::Null => "(no data)".to_string(),
        Value::Array(arr) if arr.is_empty() => "(empty)".to_string(),
        Value::Array(arr) => {
            let cols = resolve_columns(data, columns);
            if cols.is_empty() {
                // Array of scalars
                let mut lines = Vec::new();
                lines.push("| value |".to_string());
                lines.push("| --- |".to_string());
                for item in arr {
                    lines.push(format!("| {} |", escape_pipe(&value_to_cell(item))));
                }
                return lines.join("\n");
            }
            let mut lines = Vec::new();
            // Header
            let header = cols
                .iter()
                .map(|c| escape_pipe(c))
                .collect::<Vec<_>>()
                .join(" | ");
            lines.push(format!("| {} |", header));
            // Separator
            let sep = cols.iter().map(|_| "---").collect::<Vec<_>>().join(" | ");
            lines.push(format!("| {} |", sep));
            // Rows
            for item in arr {
                let row = cols
                    .iter()
                    .map(|col| escape_pipe(&value_to_cell(item.get(col).unwrap_or(&Value::Null))))
                    .collect::<Vec<_>>()
                    .join(" | ");
                lines.push(format!("| {} |", row));
            }
            lines.join("\n")
        }
        Value::Object(obj) => {
            let cols = resolve_columns(data, columns);
            let mut lines = Vec::new();
            lines.push("| key | value |".to_string());
            lines.push("| --- | --- |".to_string());
            for key in &cols {
                let v = obj.get(key).unwrap_or(&Value::Null);
                lines.push(format!(
                    "| {} | {} |",
                    escape_pipe(key),
                    escape_pipe(&value_to_cell(v))
                ));
            }
            lines.join("\n")
        }
        scalar => value_to_cell(scalar),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_array_of_objects() {
        let data = json!([{"name": "Alice", "age": 30}, {"name": "Bob", "age": 25}]);
        let out = render_markdown(&data, None);
        assert!(out.contains("| name"));
        assert!(out.contains("| ---"));
        assert!(out.contains("Alice"));
        assert!(out.contains("Bob"));
    }

    #[test]
    fn test_single_object() {
        let data = json!({"name": "Alice", "age": 30});
        let out = render_markdown(&data, None);
        assert!(out.contains("| key | value |"));
        assert!(out.contains("Alice"));
    }

    #[test]
    fn test_empty_array() {
        let data = json!([]);
        let out = render_markdown(&data, None);
        assert_eq!(out, "(empty)");
    }

    #[test]
    fn test_column_selection() {
        let data = json!([{"name": "Alice", "age": 30}, {"name": "Bob", "age": 25}]);
        let cols = vec!["name".to_string()];
        let out = render_markdown(&data, Some(&cols));
        assert!(out.contains("name"));
        assert!(out.contains("Alice"));
        assert!(!out.contains("age"));
    }
}
