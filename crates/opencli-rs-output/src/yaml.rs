use serde_json::Value;

/// Render data as YAML.
pub fn render_yaml(data: &Value, _columns: Option<&[String]>) -> String {
    match data {
        Value::Null => "~".to_string(),
        Value::Array(arr) if arr.is_empty() => "[]".to_string(),
        _ => serde_yaml::to_string(data).unwrap_or_else(|_| data.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_array_of_objects() {
        let data = json!([{"name": "Alice", "age": 30}, {"name": "Bob", "age": 25}]);
        let out = render_yaml(&data, None);
        assert!(out.contains("Alice"));
        assert!(out.contains("Bob"));
    }

    #[test]
    fn test_single_object() {
        let data = json!({"name": "Alice", "age": 30});
        let out = render_yaml(&data, None);
        assert!(out.contains("name"));
        assert!(out.contains("Alice"));
    }

    #[test]
    fn test_empty_array() {
        let data = json!([]);
        let out = render_yaml(&data, None);
        assert_eq!(out, "[]");
    }
}
