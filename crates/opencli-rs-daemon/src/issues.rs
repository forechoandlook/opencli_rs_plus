//! Issue tracker for adapters.
//!
//! DB file: ~/.opencli-rs/issues.db
//!
//! Stores user-reported problems with adapters: broken tools, wrong descriptions,
//! bad summaries, etc. Supports list / show / add / close / delete / export.
//! Future: report (push to remote registry).

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;

// ── Types ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum IssueKind {
    /// Adapter is broken — API changed, returns errors, wrong output
    Broken,
    /// Summary / description text is inaccurate or misleading
    BadDescription,
    /// Missing feature or unexpected behaviour
    Other,
}

impl std::fmt::Display for IssueKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IssueKind::Broken => write!(f, "broken"),
            IssueKind::BadDescription => write!(f, "bad_description"),
            IssueKind::Other => write!(f, "other"),
        }
    }
}

impl From<&str> for IssueKind {
    fn from(s: &str) -> Self {
        match s {
            "broken" => IssueKind::Broken,
            "bad_description" => IssueKind::BadDescription,
            _ => IssueKind::Other,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum IssueStatus {
    Open,
    Closed,
}

impl std::fmt::Display for IssueStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IssueStatus::Open => write!(f, "open"),
            IssueStatus::Closed => write!(f, "closed"),
        }
    }
}

impl From<&str> for IssueStatus {
    fn from(s: &str) -> Self {
        if s == "closed" {
            IssueStatus::Closed
        } else {
            IssueStatus::Open
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    pub id: i64,
    /// Adapter full_name, e.g. "bilibili feed"
    pub adapter: String,
    pub kind: IssueKind,
    pub title: String,
    pub body: Option<String>,
    pub status: IssueStatus,
    pub created_at: i64,
    pub updated_at: i64,
}

// ── Store ─────────────────────────────────────────────────────────────────────

pub struct IssueStore {
    conn: Mutex<Connection>,
}

impl IssueStore {
    pub fn new(db_path: PathBuf) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&db_path)?;
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS issues (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                adapter    TEXT NOT NULL,
                kind       TEXT NOT NULL DEFAULT 'other',
                title      TEXT NOT NULL,
                body       TEXT,
                status     TEXT NOT NULL DEFAULT 'open',
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_issues_adapter ON issues(adapter);
            CREATE INDEX IF NOT EXISTS idx_issues_status  ON issues(status);
            ",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn add(
        &self,
        adapter: &str,
        kind: IssueKind,
        title: &str,
        body: Option<&str>,
    ) -> Result<Issue> {
        let now = unix_now();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO issues(adapter, kind, title, body, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 'open', ?5, ?5)",
            params![adapter, kind.to_string(), title, body, now],
        )?;
        let id = conn.last_insert_rowid();
        Ok(Issue {
            id,
            adapter: adapter.to_string(),
            kind,
            title: title.to_string(),
            body: body.map(str::to_string),
            status: IssueStatus::Open,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn get(&self, id: i64) -> Result<Option<Issue>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, adapter, kind, title, body, status, created_at, updated_at
             FROM issues WHERE id = ?1",
            params![id],
            row_to_issue,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn list(
        &self,
        adapter: Option<&str>,
        status: Option<IssueStatus>,
        limit: usize,
    ) -> Result<Vec<Issue>> {
        let conn = self.conn.lock().unwrap();

        // Build query dynamically based on filters
        let mut conditions = Vec::new();
        if adapter.is_some() {
            conditions.push("adapter = ?1");
        }
        if status.is_some() {
            conditions.push("status = ?2");
        }
        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        let sql = format!(
            "SELECT id, adapter, kind, title, body, status, created_at, updated_at
             FROM issues {} ORDER BY created_at DESC LIMIT ?3",
            where_clause
        );

        let conn_ref = &conn;
        let mut stmt = conn_ref.prepare(&sql)?;

        let adapter_val = adapter.unwrap_or("");
        let status_val = status.as_ref().map(|s| s.to_string()).unwrap_or_default();

        let rows = stmt.query_map(params![adapter_val, status_val, limit as i64], row_to_issue)?;
        rows.map(|r| r.map_err(Into::into)).collect()
    }

    pub fn close(&self, id: i64) -> Result<bool> {
        let now = unix_now();
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "UPDATE issues SET status = 'closed', updated_at = ?1 WHERE id = ?2 AND status = 'open'",
            params![now, id],
        )?;
        Ok(n > 0)
    }

    pub fn delete(&self, id: i64) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute("DELETE FROM issues WHERE id = ?1", params![id])?;
        Ok(n > 0)
    }

    /// Export issues as a JSON string, optionally filtered by status.
    /// Future: add yaml/markdown formats, or push to remote registry.
    pub fn export(&self, status: Option<IssueStatus>) -> Result<String> {
        let issues = self.list(None, status, usize::MAX)?;
        Ok(serde_json::to_string_pretty(&issues)?)
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn row_to_issue(row: &rusqlite::Row<'_>) -> rusqlite::Result<Issue> {
    Ok(Issue {
        id: row.get(0)?,
        adapter: row.get(1)?,
        kind: IssueKind::from(row.get::<_, String>(2)?.as_str()),
        title: row.get(3)?,
        body: row.get(4)?,
        status: IssueStatus::from(row.get::<_, String>(5)?.as_str()),
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ── Default path ──────────────────────────────────────────────────────────────

pub fn default_issues_db_path() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".opencli-rs").join("issues.db"))
        .unwrap_or_else(|| PathBuf::from("issues.db"))
}
