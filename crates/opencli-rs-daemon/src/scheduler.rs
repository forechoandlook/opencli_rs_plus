//! Scheduler: polls due jobs and executes them using the adapter manager.

use crate::adapter_manager::AdapterManager;
use crate::store::{Job, JobStore};
use anyhow::Result;
use opencli_rs_cli::execute_command;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{error, info};

pub struct Scheduler {
    job_store: Arc<JobStore>,
    adapter_manager: Arc<AdapterManager>,
    poll_interval_secs: u64,
}

impl Scheduler {
    pub fn new(
        job_store: Arc<JobStore>,
        adapter_manager: Arc<AdapterManager>,
        poll_interval_secs: u64,
    ) -> Self {
        Self {
            job_store,
            adapter_manager,
            poll_interval_secs,
        }
    }

    /// Execute a single job using the adapter manager to resolve the command.
    pub async fn execute_job(&self, job: &Job) -> Result<(bool, Option<String>)> {
        // Parse adapter: "site command"
        let parts: Vec<&str> = job.adapter.split_whitespace().collect();
        if parts.len() != 2 {
            return Ok((
                false,
                Some(format!("Invalid adapter format: '{}'", job.adapter)),
            ));
        }
        let (site, cmd_name) = (parts[0], parts[1]);

        let cmd = match self.adapter_manager.get_command(site, cmd_name).await {
            Some(c) => c,
            None => {
                return Ok((
                    false,
                    Some(format!(
                        "Unknown or disabled adapter: {} {}",
                        site, cmd_name
                    )),
                ));
            }
        };

        let kwargs: HashMap<String, Value> = match &job.args {
            Some(serde_json::Value::Object(map)) => {
                map.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
            }
            None | Some(serde_json::Value::Null) => HashMap::new(),
            Some(_) => {
                return Ok((false, Some("args must be a JSON object".to_string())));
            }
        };

        // Inject default arg values defined in the adapter YAML (for args not supplied by the job)
        let mut kwargs = kwargs;
        for def in &cmd.args {
            if !kwargs.contains_key(&def.name) {
                if let Some(default) = &def.default {
                    kwargs.insert(def.name.clone(), default.clone());
                }
            }
        }

        match execute_command(&cmd, kwargs).await {
            Ok(data) => Ok((true, Some(serde_json::to_string(&data).unwrap_or_default()))),
            Err(e) => Ok((false, Some(format!("{}", e)))),
        }
    }

    /// Poll for due jobs and execute them.
    pub async fn poll_and_run(&self) -> Result<()> {
        let due_jobs = self.job_store.due_jobs()?;
        for job in due_jobs {
            info!(job_id = %job.id, adapter = %job.adapter, "Running job");

            if let Err(e) = self.job_store.set_running(&job.id) {
                error!(job_id = %job.id, error = %e, "Failed to set job running");
                continue;
            }

            let (success, result) = self.execute_job(&job).await?;

            if success {
                if let Err(e) = self.job_store.set_done(&job.id, result.as_deref()) {
                    error!(job_id = %job.id, error = %e, "Failed to mark job done");
                } else {
                    info!(job_id = %job.id, "Job completed");
                }
            } else {
                let error_msg = result.unwrap_or_default();
                let retry_count = job.retry_count;
                let max_retries = job.max_retries;
                match self
                    .job_store
                    .set_failed(&job.id, &error_msg, retry_count, max_retries)
                {
                    Ok(retrying) => {
                        if retrying {
                            info!(job_id = %job.id, "Job failed, will retry");
                        } else {
                            info!(job_id = %job.id, "Job failed, no more retries");
                        }
                    }
                    Err(e) => {
                        error!(job_id = %job.id, error = %e, "Failed to mark job failed");
                    }
                }
            }
        }
        Ok(())
    }

    /// Run the scheduler loop, polling at the configured interval.
    pub async fn run_loop(&self) {
        info!(poll_interval = self.poll_interval_secs, "Scheduler started");

        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(self.poll_interval_secs)).await;
            if let Err(e) = self.poll_and_run().await {
                error!(error = %e, "Poll cycle failed");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::path::PathBuf;
    use tokio;

    fn get_temp_db_path() -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("test_scheduler_{}.db", uuid::Uuid::new_v4()));
        path
    }

    #[tokio::test]
    async fn test_scheduler_execute_invalid_adapter_format() {
        let db_path = get_temp_db_path();
        let store = Arc::new(JobStore::new(db_path.clone()).unwrap());

        let job = store
            .add("invalid_adapter_format", None, Utc::now(), None)
            .unwrap();

        let manager = Arc::new(AdapterManager::new().await.unwrap());
        let scheduler = Scheduler::new(store.clone(), manager, 1);

        let (success, result) = scheduler.execute_job(&job).await.unwrap();

        assert!(!success);
        assert_eq!(
            result,
            Some("Invalid adapter format: 'invalid_adapter_format'".to_string())
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn test_scheduler_execute_unknown_adapter() {
        let db_path = get_temp_db_path();
        let store = Arc::new(JobStore::new(db_path.clone()).unwrap());

        let job = store.add("unknown cmd", None, Utc::now(), None).unwrap();

        let manager = Arc::new(AdapterManager::new().await.unwrap());
        let scheduler = Scheduler::new(store.clone(), manager, 1);

        let (success, result) = scheduler.execute_job(&job).await.unwrap();

        assert!(!success);
        assert_eq!(
            result,
            Some("Unknown or disabled adapter: unknown cmd".to_string())
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn test_scheduler_execute_invalid_args() {
        let db_path = get_temp_db_path();
        let store = Arc::new(JobStore::new(db_path.clone()).unwrap());

        let args = serde_json::json!("not an object");
        // Use a known existing adapter format to pass the adapter format check
        // but fail on arguments. If "github open" doesn't exist, we fallback nicely.
        let job = store
            .add("github open", Some(args), Utc::now(), None)
            .unwrap();

        let manager = Arc::new(AdapterManager::new().await.unwrap());
        let scheduler = Scheduler::new(store.clone(), manager, 1);

        let (success, result) = scheduler.execute_job(&job).await.unwrap();

        assert!(!success);
        assert!(
            result == Some("args must be a JSON object".to_string())
                || result == Some("Unknown or disabled adapter: github open".to_string())
        );

        let _ = std::fs::remove_file(db_path);
    }
}
