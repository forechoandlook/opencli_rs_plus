use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use urlencoding::encode;

const REPO_ISSUES_NEW_URL: &str = "https://github.com/forechoandlook/opencli_rs_plus/issues/new";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackRecord {
    pub id: String,
    pub created_at: i64,
    pub version: String,
    pub adapter: Option<String>,
    pub kind: String,
    pub title: String,
    pub body: Option<String>,
    pub opened_issue_url: Option<String>,
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn default_feedback_path() -> PathBuf {
    dirs::home_dir()
        .map(|home| home.join(".opencli-rs").join("feedback.jsonl"))
        .unwrap_or_else(|| PathBuf::from("feedback.jsonl"))
}

fn build_issue_title(adapter: Option<&str>, title: &str) -> String {
    match adapter {
        Some(adapter) if !adapter.trim().is_empty() => format!("[feedback] {adapter}: {title}"),
        _ => format!("[feedback] {title}"),
    }
}

fn build_issue_body(
    adapter: Option<&str>,
    kind: &str,
    title: &str,
    body: Option<&str>,
    record_id: &str,
) -> String {
    let mut out = String::new();
    out.push_str("## Feedback\n\n");
    out.push_str(&format!("- Version: `{}`\n", env!("CARGO_PKG_VERSION")));
    out.push_str(&format!("- Kind: `{kind}`\n"));
    if let Some(adapter) = adapter.filter(|s| !s.trim().is_empty()) {
        out.push_str(&format!("- Adapter: `{adapter}`\n"));
    }
    out.push_str(&format!("- Local record: `{record_id}`\n\n"));
    out.push_str(&format!("### Title\n{title}\n\n"));
    if let Some(body) = body.filter(|s| !s.trim().is_empty()) {
        out.push_str("### Details\n");
        out.push_str(body);
        out.push('\n');
    }
    out
}

fn build_issue_url(title: &str, body: &str) -> String {
    format!(
        "{REPO_ISSUES_NEW_URL}?title={}&body={}",
        encode(title),
        encode(body)
    )
}

pub fn save_feedback(
    adapter: Option<&str>,
    kind: &str,
    title: &str,
    body: Option<&str>,
    open_issue: bool,
) -> Result<FeedbackRecord> {
    let created_at = now_unix();
    let id = format!("fb-{created_at}-{}", std::process::id());
    let issue_title = build_issue_title(adapter, title);
    let issue_body = build_issue_body(adapter, kind, title, body, &id);
    let issue_url = open_issue.then(|| build_issue_url(&issue_title, &issue_body));

    let record = FeedbackRecord {
        id,
        created_at,
        version: env!("CARGO_PKG_VERSION").to_string(),
        adapter: adapter.map(str::to_string),
        kind: kind.to_string(),
        title: title.to_string(),
        body: body.map(str::to_string),
        opened_issue_url: issue_url.clone(),
    };

    let path = default_feedback_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    writeln!(file, "{}", serde_json::to_string(&record)?)
        .with_context(|| format!("failed to write {}", path.display()))?;

    println!("Feedback saved: {}", path.display());
    println!("Record id: {}", record.id);

    if let Some(url) = issue_url {
        webbrowser::open(&url).context("failed to open browser for GitHub issue")?;
        println!("Opened issue form: {}", url);
    }

    Ok(record)
}
