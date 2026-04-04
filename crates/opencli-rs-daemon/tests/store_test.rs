use opencli_rs_daemon::store::{JobStore, JobStatus};
use chrono::Utc;
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
    assert_eq!(job.status, JobStatus::Pending);

    let _ = fs::remove_file(db_path);
}
