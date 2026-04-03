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

    pub fn add(
        &self,
        adapter: &str,
        args: Option<serde_json::Value>,
        run_at: DateTime<Utc>,
        interval_seconds: Option<i64>,
    ) -> Result<Job> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let status = JobStatus::Pending;
        let status_str = status.to_string();

        let args_str = args
            .as_ref()
            .map(|a| serde_json::to_string(a).unwrap_or_default());
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

        let job = stmt
            .query_row(params![id, format!("{}%", id)], |row| {
                Ok(Job {
                    id: row.get(0)?,
                    adapter: row.get(1)?,
                    args: row
                        .get::<_, Option<String>>(2)?
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
                    start_at: row.get::<_, Option<String>>(10)?.and_then(|s| {
                        DateTime::parse_from_rfc3339(&s)
                            .map(|dt| dt.with_timezone(&Utc))
                            .ok()
                    }),
                    end_at: row.get::<_, Option<String>>(11)?.and_then(|s| {
                        DateTime::parse_from_rfc3339(&s)
                            .map(|dt| dt.with_timezone(&Utc))
                            .ok()
                    }),
                    created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(12)?)
                        .unwrap_or_default()
                        .with_timezone(&Utc),
                    updated_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(13)?)
                        .unwrap_or_default()
                        .with_timezone(&Utc),
                })
            })
            .optional()?;

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
                args: row
                    .get::<_, Option<String>>(2)?
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
                start_at: row.get::<_, Option<String>>(10)?.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .map(|dt| dt.with_timezone(&Utc))
                        .ok()
                }),
                end_at: row.get::<_, Option<String>>(11)?.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .map(|dt| dt.with_timezone(&Utc))
                        .ok()
                }),
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
             ORDER BY run_at ASC",
        )?;

        let rows = stmt.query_map(params![now], |row| {
            Ok(Job {
                id: row.get(0)?,
                adapter: row.get(1)?,
                args: row
                    .get::<_, Option<String>>(2)?
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
                start_at: row.get::<_, Option<String>>(10)?.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .map(|dt| dt.with_timezone(&Utc))
                        .ok()
                }),
                end_at: row.get::<_, Option<String>>(11)?.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .map(|dt| dt.with_timezone(&Utc))
                        .ok()
                }),
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

        let interval: Option<i64> = conn.query_row(
            "SELECT interval_seconds FROM jobs WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )?;

        let next_run_at = interval
            .filter(|&i| i > 0)
            .map(|i| now + chrono::Duration::seconds(i));

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

    pub fn set_failed(
        &self,
        id: &str,
        error: &str,
        retry_count: i32,
        max_retries: i32,
    ) -> Result<bool> {
        let now = Utc::now();
        // Since retry_count is 0-indexed, if it's equal to or greater than max_retries we shouldn't retry anymore
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use std::env;
    use std::fs;

    fn get_temp_db_path() -> PathBuf {
        let mut path = env::temp_dir();
        path.push(format!("test_store_{}.db", uuid::Uuid::new_v4()));
        path
    }

    #[test]
    fn test_store_creation_and_add() {
        let db_path = get_temp_db_path();
        let store = JobStore::new(db_path.clone()).unwrap();

        let args = serde_json::json!({"param": "value"});
        let run_at = Utc::now();
        let job = store
            .add("test_adapter", Some(args.clone()), run_at, Some(60))
            .unwrap();

        assert_eq!(job.adapter, "test_adapter");
        assert_eq!(job.args, Some(args));
        assert_eq!(job.interval_seconds, Some(60));
        assert_eq!(job.status, JobStatus::Pending);

        // Get the job back
        let fetched = store.get(&job.id).unwrap().unwrap();
        assert_eq!(fetched.id, job.id);
        assert_eq!(fetched.adapter, "test_adapter");
        assert_eq!(fetched.status, JobStatus::Pending);

        let _ = fs::remove_file(db_path);
    }

    #[test]
    fn test_store_list_and_due() {
        let db_path = get_temp_db_path();
        let store = JobStore::new(db_path.clone()).unwrap();

        let now = Utc::now();
        let past = now - Duration::seconds(60);
        let future = now + Duration::seconds(60);

        // Add 3 jobs
        let j1 = store.add("past_job", None, past, None).unwrap();
        let _j2 = store.add("future_job", None, future, None).unwrap();
        let j3 = store
            .add("another_past", None, past - Duration::seconds(10), None)
            .unwrap();

        // List all
        let all = store.list(None, 10).unwrap();
        assert_eq!(all.len(), 3);

        // List due
        let due = store.due_jobs().unwrap();
        assert_eq!(due.len(), 2);

        // Order should be ascending by run_at, so j3 should be first
        assert_eq!(due[0].id, j3.id);
        assert_eq!(due[1].id, j1.id);

        let _ = fs::remove_file(db_path);
    }

    #[test]
    fn test_store_status_updates() {
        let db_path = get_temp_db_path();
        let store = JobStore::new(db_path.clone()).unwrap();

        let job = store.add("test_job", None, Utc::now(), None).unwrap();

        // Set running
        store.set_running(&job.id).unwrap();
        let running_job = store.get(&job.id).unwrap().unwrap();
        assert_eq!(running_job.status, JobStatus::Running);
        assert!(running_job.start_at.is_some());

        // Set done
        store.set_done(&job.id, Some("success")).unwrap();
        let done_job = store.get(&job.id).unwrap().unwrap();
        assert_eq!(done_job.status, JobStatus::Done);
        assert_eq!(done_job.result.as_deref(), Some("success"));
        assert!(done_job.end_at.is_some());

        let _ = fs::remove_file(db_path);
    }

    #[test]
    fn test_store_failed_and_retry() {
        let db_path = get_temp_db_path();
        let store = JobStore::new(db_path.clone()).unwrap();

        let job = store.add("test_job", None, Utc::now(), None).unwrap();

        // Fail it once (should retry)
        let should_retry = store.set_failed(&job.id, "error 1", 0, 3).unwrap();
        assert!(should_retry);

        let retry_job = store.get(&job.id).unwrap().unwrap();
        assert_eq!(retry_job.status, JobStatus::Pending); // Returns to pending
        assert_eq!(retry_job.error.as_deref(), Some("error 1"));
        assert_eq!(retry_job.retry_count, 1);

        // Fail it again
        store.set_failed(&job.id, "error 2", 1, 3).unwrap();
        let should_retry_last = store.set_failed(&job.id, "error 3", 3, 3).unwrap();
        assert!(!should_retry_last);

        let failed_job = store.get(&job.id).unwrap().unwrap();
        assert_eq!(failed_job.status, JobStatus::Failed);
        assert_eq!(failed_job.error.as_deref(), Some("error 3"));
        assert_eq!(failed_job.retry_count, 4);

        let _ = fs::remove_file(db_path);
    }

    #[test]
    fn test_store_cancel_and_delete() {
        let db_path = get_temp_db_path();
        let store = JobStore::new(db_path.clone()).unwrap();

        let job = store.add("test_job", None, Utc::now(), None).unwrap();

        store.cancel(&job.id).unwrap();
        let cancelled = store.get(&job.id).unwrap().unwrap();
        assert_eq!(cancelled.status, JobStatus::Cancelled);

        store.delete(&job.id).unwrap();
        let deleted = store.get(&job.id).unwrap();
        assert!(deleted.is_none());

        let _ = fs::remove_file(db_path);
    }
}
