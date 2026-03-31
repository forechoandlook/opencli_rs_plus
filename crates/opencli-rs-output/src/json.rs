use serde_json::Value;

/// Render data as pretty-printed JSON.
pub fn render_json(data: &Value, _columns: Option<&[String]>) -> String {
    match data {
        Value::Null => "null".to_string(),
        _ => serde_json::to_string_pretty(data).unwrap_or_else(|_| data.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_array_of_objects() {
        let data = json!([{"name": "Alice", "age": 30}, {"name": "Bob", "age": 25}]);
        let out = render_json(&data, None);
        assert!(out.contains("Alice"));
        assert!(out.contains("Bob"));
        // Should be pretty-printed (contains newlines)
        assert!(out.contains('\n'));
    }

    #[test]
    fn test_single_object() {
        let data = json!({"name": "Alice", "age": 30});
        let out = render_json(&data, None);
        assert!(out.contains("Alice"));
        assert!(out.contains("30"));
    }

    #[test]
    fn test_empty_array() {
        let data = json!([]);
        let out = render_json(&data, None);
        assert_eq!(out, "[]");
    }
}
