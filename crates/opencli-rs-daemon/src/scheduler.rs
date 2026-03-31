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
    pub fn new(job_store: Arc<JobStore>, adapter_manager: Arc<AdapterManager>, poll_interval_secs: u64) -> Self {
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
            return Ok((false, Some(format!("Invalid adapter format: '{}'", job.adapter))));
        }
        let (site, cmd_name) = (parts[0], parts[1]);

        let cmd = match self.adapter_manager.get_command(site, cmd_name).await {
            Some(c) => c,
            None => {
                return Ok((
                    false,
                    Some(format!("Unknown or disabled adapter: {} {}", site, cmd_name)),
                ));
            }
        };

        let kwargs: HashMap<String, Value> = match &job.args {
            Some(serde_json::Value::Object(map)) => {
                map.iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect()
            }
            Some(_) => {
                return Ok((false, Some("args must be a JSON object".to_string())));
            }
            None => HashMap::new(),
        };

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
        info!(
            poll_interval = self.poll_interval_secs,
            "Scheduler started"
        );

        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(self.poll_interval_secs)).await;
            if let Err(e) = self.poll_and_run().await {
                error!(error = %e, "Poll cycle failed");
            }
        }
    }
}
