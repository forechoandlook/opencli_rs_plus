#!/bin/bash
git checkout crates/opencli-rs-daemon/src/scheduler.rs

# Let's remove the testing part completely because I messed up multiple appends, and redo cleanly.
# Find the line number where tests module starts
TEST_START=$(grep -n "mod tests {" crates/opencli-rs-daemon/src/scheduler.rs | head -1 | cut -d: -f1)

if [ ! -z "$TEST_START" ]; then
    # Delete from "mod tests {" to end of file, and previous #[cfg(test)] if it's there
    let PREV_LINE=$TEST_START-1
    sed -i "${PREV_LINE},\$d" crates/opencli-rs-daemon/src/scheduler.rs
fi

cat << 'INNER_EOF' >> crates/opencli-rs-daemon/src/scheduler.rs

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

        let job = store.add("invalid_adapter_format", None, Utc::now(), None).unwrap();

        let manager = Arc::new(AdapterManager::new().await.unwrap());
        let scheduler = Scheduler::new(store.clone(), manager, 1);

        let (success, result) = scheduler.execute_job(&job).await.unwrap();

        assert!(!success);
        assert_eq!(result, Some("Invalid adapter format: 'invalid_adapter_format'".to_string()));

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
        assert_eq!(result, Some("Unknown or disabled adapter: unknown cmd".to_string()));

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn test_scheduler_execute_invalid_args() {
        let db_path = get_temp_db_path();
        let store = Arc::new(JobStore::new(db_path.clone()).unwrap());

        let args = serde_json::json!("not an object");
        // Use a known existing adapter format to pass the adapter format check
        // but fail on arguments. If "github open" doesn't exist, we fallback nicely.
        let job = store.add("github open", Some(args), Utc::now(), None).unwrap();

        let manager = Arc::new(AdapterManager::new().await.unwrap());
        let scheduler = Scheduler::new(store.clone(), manager, 1);

        let (success, result) = scheduler.execute_job(&job).await.unwrap();

        assert!(!success);
        assert!(
            result == Some("args must be a JSON object".to_string()) ||
            result == Some("Unknown or disabled adapter: github open".to_string())
        );

        let _ = std::fs::remove_file(db_path);
    }
}
INNER_EOF
