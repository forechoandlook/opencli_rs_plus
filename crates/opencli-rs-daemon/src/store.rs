use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Pending,
    Running,
    Done,
    Failed,
    Cancelled,
}

impl std::fmt::Display for JobStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JobStatus::Pending => write!(f, "pending"),
            JobStatus::Running => write!(f, "running"),
            JobStatus::Done => write!(f, "done"),
            JobStatus::Failed => write!(f, "failed"),
            JobStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl From<&str> for JobStatus {
    fn from(s: &str) -> Self {
        match s {
            "pending" => JobStatus::Pending,
            "running" => JobStatus::Running,
            "done" => JobStatus::Done,
            "failed" => JobStatus::Failed,
            "cancelled" => JobStatus::Cancelled,
            _ => JobStatus::Pending,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub adapter: String,
    pub args: Option<serde_json::Value>,
    pub run_at: DateTime<Utc>,
    pub interval_seconds: Option<i64>,
    pub status: JobStatus,
    pub retry_count: i32,
    pub max_retries: i32,
    pub result: Option<String>,
    pub error: Option<String>,
    pub start_at: Option<DateTime<Utc>>,
    pub end_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub struct JobStore {
    conn: Mutex<Connection>,
}

impl JobStore {
    pub fn new(db_path: PathBuf) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&db_path)?;
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS jobs (
                id TEXT PRIMARY KEY,
                adapter TEXT NOT NULL,
                args TEXT,
                run_at TEXT NOT NULL,
                interval_seconds INTEGER,
                status TEXT DEFAULT 'pending',
                retry_count INTEGER DEFAULT 0,
                max_retries INTEGER DEFAULT 3,
                result TEXT,
                error TEXT,
                start_at TEXT,
                end_at TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_jobs_status_run_at ON jobs(status, run_at);
            ",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn add(&self, adapter: &str, args: Option<serde_json::Value>, run_at: DateTime<Utc>, interval_seconds: Option<i64>) -> Result<Job> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let status = JobStatus::Pending;
        let status_str = status.to_string();

        let args_str = args.as_ref().map(|a| serde_json::to_string(a).unwrap_or_default());
        let interval = interval_seconds.unwrap_or(0);

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO jobs (id, adapter, args, run_at, interval_seconds, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                &id,
                adapter,
                args_str,
                run_at.to_rfc3339(),
                interval,
                status_str,
                now.to_rfc3339(),
                now.to_rfc3339()
            ],
        )?;

        drop(conn);
        Ok(Job {
            id,
            adapter: adapter.to_string(),
            args,
            run_at,
            interval_seconds,
            status,
            retry_count: 0,
            max_retries: 3,
            result: None,
            error: None,
            start_at: None,
            end_at: None,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn get(&self, id: &str) -> Result<Option<Job>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, adapter, args, run_at, interval_seconds, status, retry_count, max_retries,
                    result, error, start_at, end_at, created_at, updated_at FROM jobs WHERE id = ?1 OR id LIKE ?2"
        )?;

        let job = stmt.query_row(params![id, format!("{}%", id)], |row| {
            Ok(Job {
                id: row.get(0)?,
                adapter: row.get(1)?,
                args: row.get::<_, Option<String>>(2)?
                    .and_then(|s| serde_json::from_str(&s).ok()),
                run_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(3)?)
                    .unwrap_or_default()
                    .with_timezone(&Utc),
                interval_seconds: row.get::<_, Option<i64>>(4)?,
                status: JobStatus::from(row.get::<_, String>(5)?.as_str()),
                retry_count: row.get(6)?,
                max_retries: row.get(7)?,
                result: row.get(8)?,
                error: row.get(9)?,
                start_at: row.get::<_, Option<String>>(10)?
                    .map(|s| DateTime::parse_from_rfc3339(&s).map(|dt| dt.with_timezone(&Utc)).ok())
                    .flatten(),
                end_at: row.get::<_, Option<String>>(11)?
                    .map(|s| DateTime::parse_from_rfc3339(&s).map(|dt| dt.with_timezone(&Utc)).ok())
                    .flatten(),
                created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(12)?)
                    .unwrap_or_default()
                    .with_timezone(&Utc),
                updated_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(13)?)
                    .unwrap_or_default()
                    .with_timezone(&Utc),
            })
        }).optional()?;

        Ok(job)
    }

    pub fn list(&self, status_filter: Option<JobStatus>, limit: usize) -> Result<Vec<Job>> {
        let conn = self.conn.lock().unwrap();
        let query = match &status_filter {
            Some(s) => format!(
                "SELECT id, adapter, args, run_at, interval_seconds, status, retry_count, max_retries,
                        result, error, start_at, end_at, created_at, updated_at
                 FROM jobs WHERE status = '{}' ORDER BY created_at DESC LIMIT {}",
                s, limit
            ),
            None => format!(
                "SELECT id, adapter, args, run_at, interval_seconds, status, retry_count, max_retries,
                        result, error, start_at, end_at, created_at, updated_at
                 FROM jobs ORDER BY created_at DESC LIMIT {}",
                limit
            ),
        };

        let mut stmt = conn.prepare(&query)?;
        let rows = stmt.query_map([], |row| {
            Ok(Job {
                id: row.get(0)?,
                adapter: row.get(1)?,
                args: row.get::<_, Option<String>>(2)?
                    .and_then(|s| serde_json::from_str(&s).ok()),
                run_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(3)?)
                    .unwrap_or_default()
                    .with_timezone(&Utc),
                interval_seconds: row.get::<_, Option<i64>>(4)?,
                status: JobStatus::from(row.get::<_, String>(5)?.as_str()),
                retry_count: row.get(6)?,
                max_retries: row.get(7)?,
                result: row.get(8)?,
                error: row.get(9)?,
                start_at: row.get::<_, Option<String>>(10)?
                    .map(|s| DateTime::parse_from_rfc3339(&s).map(|dt| dt.with_timezone(&Utc)).ok())
                    .flatten(),
                end_at: row.get::<_, Option<String>>(11)?
                    .map(|s| DateTime::parse_from_rfc3339(&s).map(|dt| dt.with_timezone(&Utc)).ok())
                    .flatten(),
                created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(12)?)
                    .unwrap_or_default()
                    .with_timezone(&Utc),
                updated_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(13)?)
                    .unwrap_or_default()
                    .with_timezone(&Utc),
            })
        })?;

        let mut jobs = Vec::new();
        for job in rows {
            jobs.push(job?);
        }
        Ok(jobs)
    }

    pub fn due_jobs(&self) -> Result<Vec<Job>> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        let mut stmt = conn.prepare(
            "SELECT id, adapter, args, run_at, interval_seconds, status, retry_count, max_retries,
                    result, error, start_at, end_at, created_at, updated_at
             FROM jobs
             WHERE status = 'pending' AND run_at <= ?1
             ORDER BY run_at ASC"
        )?;

        let rows = stmt.query_map(params![now], |row| {
            Ok(Job {
                id: row.get(0)?,
                adapter: row.get(1)?,
                args: row.get::<_, Option<String>>(2)?
                    .and_then(|s| serde_json::from_str(&s).ok()),
                run_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(3)?)
                    .unwrap_or_default()
                    .with_timezone(&Utc),
                interval_seconds: row.get::<_, Option<i64>>(4)?,
                status: JobStatus::from(row.get::<_, String>(5)?.as_str()),
                retry_count: row.get(6)?,
                max_retries: row.get(7)?,
                result: row.get(8)?,
                error: row.get(9)?,
                start_at: row.get::<_, Option<String>>(10)?
                    .map(|s| DateTime::parse_from_rfc3339(&s).map(|dt| dt.with_timezone(&Utc)).ok())
                    .flatten(),
                end_at: row.get::<_, Option<String>>(11)?
                    .map(|s| DateTime::parse_from_rfc3339(&s).map(|dt| dt.with_timezone(&Utc)).ok())
                    .flatten(),
                created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(12)?)
                    .unwrap_or_default()
                    .with_timezone(&Utc),
                updated_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(13)?)
                    .unwrap_or_default()
                    .with_timezone(&Utc),
            })
        })?;

        let mut jobs = Vec::new();
        for job in rows {
            jobs.push(job?);
        }
        Ok(jobs)
    }

    pub fn set_running(&self, id: &str) -> Result<()> {
        let now = Utc::now();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE jobs SET status = 'running', start_at = ?1, updated_at = ?1 WHERE id = ?2",
            params![now.to_rfc3339(), id],
        )?;
        Ok(())
    }

    pub fn set_done(&self, id: &str, result: Option<&str>) -> Result<Option<DateTime<Utc>>> {
        let now = Utc::now();
        let conn = self.conn.lock().unwrap();
        let next_run_at: Option<DateTime<Utc>> = conn.query_row(
            "SELECT interval_seconds FROM jobs WHERE id = ?1",
            params![id],
            |row| row.get::<_, Option<i64>>(0).map(|opt| opt.map(|i| now + chrono::Duration::seconds(i)))
        )?;

        if let Some(next) = next_run_at {
            // Reschedule for next interval
            conn.execute(
                "UPDATE jobs SET status = 'pending', result = ?1, error = NULL, end_at = ?2,
                 run_at = ?3, updated_at = ?2 WHERE id = ?4",
                params![result, now.to_rfc3339(), next.to_rfc3339(), id],
            )?;
        } else {
            conn.execute(
                "UPDATE jobs SET status = 'done', result = ?1, error = NULL, end_at = ?2, updated_at = ?2 WHERE id = ?3",
                params![result, now.to_rfc3339(), id],
            )?;
        }
        Ok(next_run_at)
    }

    pub fn set_failed(&self, id: &str, error: &str, retry_count: i32, max_retries: i32) -> Result<bool> {
        let now = Utc::now();
        let should_retry = retry_count < max_retries;
        let status = if should_retry { "pending" } else { "failed" };
        let next_run = if should_retry {
            // Exponential backoff: 2^retry * 30 seconds
            let delay_secs = 30 * 2_i64.pow(retry_count as u32);
            (now + chrono::Duration::seconds(delay_secs)).to_rfc3339()
        } else {
            now.to_rfc3339()
        };

        let conn = self.conn.lock().unwrap();
        if should_retry {
            conn.execute(
                "UPDATE jobs SET status = ?1, error = ?2, retry_count = ?3, run_at = ?4, updated_at = ?4 WHERE id = ?5",
                params![status, error, retry_count + 1, next_run, id],
            )?;
        } else {
            conn.execute(
                "UPDATE jobs SET status = ?1, error = ?2, retry_count = ?3, end_at = ?4, updated_at = ?4 WHERE id = ?5",
                params![status, error, retry_count + 1, now.to_rfc3339(), id],
            )?;
        }
        Ok(should_retry)
    }

    pub fn cancel(&self, id: &str) -> Result<()> {
        let now = Utc::now();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE jobs SET status = 'cancelled', updated_at = ?1 WHERE id = ?2 AND status IN ('pending', 'running')",
            params![now.to_rfc3339(), id],
        )?;
        Ok(())
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM jobs WHERE id = ?1", params![id])?;
        Ok(())
    }
}

trait OptionalExt<T> {
    fn optional(self) -> Result<Option<T>, rusqlite::Error>;
}

impl<T> OptionalExt<T> for Result<T, rusqlite::Error> {
    fn optional(self) -> Result<Option<T>, rusqlite::Error> {
        match self {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}
