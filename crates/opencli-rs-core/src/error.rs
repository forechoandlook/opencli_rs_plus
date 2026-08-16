use thiserror::Error;

#[derive(Debug, Error)]
pub enum CliError {
    #[error("[browser] {message}")]
    BrowserConnect {
        message: String,
        suggestions: Vec<String>,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("[adapter] {message}")]
    AdapterLoad {
        message: String,
        suggestions: Vec<String>,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("[command] {message}")]
    CommandExecution {
        message: String,
        suggestions: Vec<String>,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("[config] {message}")]
    Config {
        message: String,
        suggestions: Vec<String>,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("[auth] {message}")]
    AuthRequired {
        message: String,
        suggestions: Vec<String>,
    },

    #[error("[timeout] {message}")]
    Timeout {
        message: String,
        suggestions: Vec<String>,
    },

    #[error("[argument] {message}")]
    Argument {
        message: String,
        suggestions: Vec<String>,
    },

    #[error("[empty] {message}")]
    EmptyResult {
        message: String,
        suggestions: Vec<String>,
    },

    #[error("[selector] {message}")]
    Selector {
        message: String,
        suggestions: Vec<String>,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("[pipeline] {message}")]
    Pipeline {
        message: String,
        suggestions: Vec<String>,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("[io] {0}")]
    Io(#[from] std::io::Error),

    #[error("[json] {0}")]
    Json(#[from] serde_json::Error),

    #[error("[yaml] {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("[http] {message}")]
    Http {
        message: String,
        suggestions: Vec<String>,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("[gone] {message}")]
    Gone {
        message: String,
        suggestions: Vec<String>,
    },

    #[error("[rate_limit] {message}")]
    RateLimit {
        message: String,
        suggestions: Vec<String>,
    },
}

impl CliError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::BrowserConnect { .. } => "BROWSER_CONNECT",
            Self::AdapterLoad { .. } => "ADAPTER_LOAD",
            Self::CommandExecution { .. } => "COMMAND_EXECUTION",
            Self::Config { .. } => "CONFIG",
            Self::AuthRequired { .. } => "AUTH_REQUIRED",
            Self::Timeout { .. } => "TIMEOUT",
            Self::Argument { .. } => "ARGUMENT",
            Self::EmptyResult { .. } => "EMPTY_RESULT",
            Self::Selector { .. } => "SELECTOR",
            Self::Pipeline { .. } => "PIPELINE",
            Self::Io(_) => "IO",
            Self::Json(_) => "JSON",
            Self::Yaml(_) => "YAML",
            Self::Http { .. } => "HTTP",
            Self::Gone { .. } => "GONE",
            Self::RateLimit { .. } => "RATE_LIMIT",
        }
    }

    /// Process exit code. Soft empties (`EMPTY_RESULT`) are 0 so batch jobs
    /// can treat “no rows” as success. `GONE` is 2, auth is 3, rate-limit is 4.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::EmptyResult { .. } => 0,
            Self::Gone { .. } => 2,
            Self::AuthRequired { .. } => 3,
            Self::RateLimit { .. } => 4,
            Self::Argument { .. } => 2,
            _ => 1,
        }
    }

    pub fn is_soft_empty(&self) -> bool {
        matches!(self, Self::EmptyResult { .. })
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Self::BrowserConnect { .. } => "🌐",
            Self::AdapterLoad { .. } => "🔌",
            Self::CommandExecution { .. } => "⚡",
            Self::Config { .. } => "⚙️",
            Self::AuthRequired { .. } => "🔒",
            Self::Timeout { .. } => "⏱️",
            Self::Argument { .. } => "📝",
            Self::EmptyResult { .. } => "📭",
            Self::Selector { .. } => "🎯",
            Self::Pipeline { .. } => "🔧",
            Self::Io(_) => "💾",
            Self::Json(_) => "📄",
            Self::Yaml(_) => "📄",
            Self::Http { .. } => "🌍",
            Self::Gone { .. } => "👻",
            Self::RateLimit { .. } => "🛑",
        }
    }

    pub fn suggestions(&self) -> &[String] {
        match self {
            Self::BrowserConnect { suggestions, .. }
            | Self::AdapterLoad { suggestions, .. }
            | Self::CommandExecution { suggestions, .. }
            | Self::Config { suggestions, .. }
            | Self::AuthRequired { suggestions, .. }
            | Self::Timeout { suggestions, .. }
            | Self::Argument { suggestions, .. }
            | Self::EmptyResult { suggestions, .. }
            | Self::Selector { suggestions, .. }
            | Self::Pipeline { suggestions, .. }
            | Self::Http { suggestions, .. }
            | Self::Gone { suggestions, .. }
            | Self::RateLimit { suggestions, .. } => suggestions,
            Self::Io(_) | Self::Json(_) | Self::Yaml(_) => &[],
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "ok": false,
            "code": self.code(),
            "error": self.to_string(),
            "suggestions": self.suggestions(),
        })
    }

    /// Reclassify a pipeline/command error using prefixes adapters throw from JS.
    pub fn classify(self) -> Self {
        let msg = self.to_string();
        if let Some(mapped) = classify_js_error(&msg) {
            return mapped;
        }
        self
    }

    // Convenience constructors

    pub fn browser_connect(msg: impl Into<String>) -> Self {
        Self::BrowserConnect {
            message: msg.into(),
            suggestions: vec![],
            source: None,
        }
    }

    pub fn argument(msg: impl Into<String>) -> Self {
        Self::Argument {
            message: msg.into(),
            suggestions: vec![],
        }
    }

    pub fn timeout(msg: impl Into<String>) -> Self {
        Self::Timeout {
            message: msg.into(),
            suggestions: vec![],
        }
    }

    pub fn auth_required(msg: impl Into<String>) -> Self {
        Self::AuthRequired {
            message: msg.into(),
            suggestions: vec![],
        }
    }

    pub fn empty_result(msg: impl Into<String>) -> Self {
        Self::EmptyResult {
            message: msg.into(),
            suggestions: vec![],
        }
    }

    pub fn command_execution(msg: impl Into<String>) -> Self {
        Self::CommandExecution {
            message: msg.into(),
            suggestions: vec![],
            source: None,
        }
    }

    pub fn pipeline(msg: impl Into<String>) -> Self {
        Self::Pipeline {
            message: msg.into(),
            suggestions: vec![],
            source: None,
        }
    }

    pub fn gone(msg: impl Into<String>) -> Self {
        Self::Gone {
            message: msg.into(),
            suggestions: vec![],
        }
    }

    pub fn rate_limit(msg: impl Into<String>) -> Self {
        Self::RateLimit {
            message: msg.into(),
            suggestions: vec![],
        }
    }
}

/// Map adapter JS `throw new Error("CODE: ...")` (and common Chinese site
/// messages) onto typed errors.
pub fn classify_js_error(message: &str) -> Option<CliError> {
    let upper = message.to_ascii_uppercase();
    if upper.contains("AUTH_REQUIRED") || message.contains("请确认已登录") || message.contains("未登录")
    {
        return Some(CliError::auth_required(message));
    }
    if upper.contains("RATE_LIMIT")
        || message.contains("429")
        || message.contains("频繁")
        || message.contains("操作太快")
    {
        return Some(CliError::rate_limit(message));
    }
    if upper.contains("GONE")
        || message.contains("已注销")
        || message.contains("不存在")
        || message.contains("账号已重置")
    {
        return Some(CliError::gone(message));
    }
    if upper.contains("EMPTY")
        || message.contains("没有可读取")
        || message.contains("没有可读取的公开")
        || message.contains("当前页无数据")
    {
        return Some(CliError::empty_result(message));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_auth_and_gone() {
        let err = classify_js_error("Error: AUTH_REQUIRED: zhihu.com").unwrap();
        assert_eq!(err.code(), "AUTH_REQUIRED");
        let gone = classify_js_error("知乎接口失败: 该账号已注销").unwrap();
        assert_eq!(gone.code(), "GONE");
        let empty = classify_js_error("该用户没有可读取的公开内容").unwrap();
        assert_eq!(empty.code(), "EMPTY_RESULT");
    }

    #[test]
    fn empty_exits_zero() {
        assert_eq!(CliError::empty_result("none").exit_code(), 0);
        assert_eq!(CliError::gone("x").exit_code(), 2);
    }
}
