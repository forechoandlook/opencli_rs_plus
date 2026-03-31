use serde_json::Value;

pub trait ValueExt {
    fn as_str_or_default(&self) -> &str;
    fn get_path(&self, path: &str) -> Option<&Value>;
    fn is_empty_result(&self) -> bool;
    fn to_array(&self) -> Vec<&Value>;
}

impl ValueExt for Value {
    fn as_str_or_default(&self) -> &str {
        match self {
            Value::String(s) => s.as_str(),
            Value::Null => "",
            _ => "",
        }
    }

    fn get_path(&self, path: &str) -> Option<&Value> {
        let mut current = self;
        for segment in path.split('.') {
            // Try as object key first
            if let Some(val) = current.get(segment) {
                current = val;
            } else if let Ok(index) = segment.parse::<usize>() {
                // Try as array index
                current = current.get(index)?;
            } else {
                return None;
            }
        }
        Some(current)
    }

    fn is_empty_result(&self) -> bool {
        match self {
            Value::Null => true,
            Value::Array(arr) => arr.is_empty(),
            Value::Object(obj) => obj.is_empty(),
            Value::String(s) => s.is_empty(),
            _ => false,
        }
    }

    fn to_array(&self) -> Vec<&Value> {
        match self {
            Value::Array(arr) => arr.iter().collect(),
            Value::Null => vec![],
            other => vec![other],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_as_str_or_default() {
        assert_eq!(json!("hello").as_str_or_default(), "hello");
        assert_eq!(json!(null).as_str_or_default(), "");
        assert_eq!(json!(42).as_str_or_default(), "");
    }

    #[test]
    fn test_get_path() {
        let val = json!({"a": {"b": {"c": 42}}});
        assert_eq!(val.get_path("a.b.c"), Some(&json!(42)));
        assert_eq!(val.get_path("a.b.d"), None);
    }

    #[test]
    fn test_get_path_array_index() {
        let val = json!({"items": [10, 20, 30]});
        assert_eq!(val.get_path("items.1"), Some(&json!(20)));
    }

    #[test]
    fn test_is_empty_result() {
        assert!(json!(null).is_empty_result());
        assert!(json!([]).is_empty_result());
        assert!(json!({}).is_empty_result());
        assert!(json!("").is_empty_result());
        assert!(!json!([1]).is_empty_result());
        assert!(!json!(42).is_empty_result());
    }

    #[test]
    fn test_to_array() {
        assert_eq!(json!([1, 2]).to_array().len(), 2);
        assert_eq!(json!(null).to_array().len(), 0);
        assert_eq!(json!(42).to_array().len(), 1);
    }
}
