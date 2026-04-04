#!/bin/bash
git checkout crates/opencli-rs-daemon/tests/store_test.rs

cat << 'INNER_EOF' > crates/opencli-rs-daemon/tests/store_test.rs
use opencli_rs_daemon::store::{JobStore, JobStatus};
use chrono::{Utc, Duration};
use std::path::PathBuf;
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
    let job = store.add("test_adapter", Some(args.clone()), run_at, Some(60)).unwrap();

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
    let j3 = store.add("another_past", None, past - Duration::seconds(10), None).unwrap();

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

    assert_eq!(done_job.status, JobStatus::Pending);
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

    store.set_failed(&job.id, "error 2", 1, 3).unwrap();
    store.set_failed(&job.id, "error 3", 2, 3).unwrap();
    let should_retry_last = store.set_failed(&job.id, "error 4", 3, 3).unwrap();
    assert!(!should_retry_last);

    let failed_job = store.get(&job.id).unwrap().unwrap();
    assert_eq!(failed_job.status, JobStatus::Failed);
    assert_eq!(failed_job.error.as_deref(), Some("error 4"));

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
INNER_EOF
