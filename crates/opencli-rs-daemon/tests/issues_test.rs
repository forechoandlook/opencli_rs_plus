use opencli_rs_daemon::issues::{IssueKind, IssueStatus, IssueStore};
use std::path::PathBuf;

fn temp_db() -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("test_issues_{}.db", uuid::Uuid::new_v4()));
    path
}

fn cleanup(path: &PathBuf) {
    let _ = std::fs::remove_file(path);
}

// ─────────────────────────────────────────────────────────────────────────────
// 基础 CRUD
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_issue_add_returns_correct_fields() {
    let path = temp_db();
    let store = IssueStore::new(path.clone()).unwrap();

    let issue = store
        .add(
            "bilibili feed",
            IssueKind::Broken,
            "API 返回 404",
            Some("接口已下线"),
        )
        .unwrap();

    assert_eq!(issue.adapter, "bilibili feed");
    assert_eq!(issue.kind, IssueKind::Broken);
    assert_eq!(issue.title, "API 返回 404");
    assert_eq!(issue.body.as_deref(), Some("接口已下线"));
    assert_eq!(issue.status, IssueStatus::Open);
    assert!(issue.id > 0);
    assert!(issue.created_at > 0);
    assert_eq!(issue.created_at, issue.updated_at);

    cleanup(&path);
}

#[test]
fn test_issue_add_without_body() {
    let path = temp_db();
    let store = IssueStore::new(path.clone()).unwrap();

    let issue = store
        .add("test adapter", IssueKind::Other, "title only", None)
        .unwrap();

    assert!(issue.body.is_none());
    cleanup(&path);
}

#[test]
fn test_issue_get_returns_inserted_issue() {
    let path = temp_db();
    let store = IssueStore::new(path.clone()).unwrap();

    let created = store
        .add("zhihu hot", IssueKind::BadDescription, "描述不准确", None)
        .unwrap();

    let fetched = store.get(created.id).unwrap().unwrap();
    assert_eq!(fetched.id, created.id);
    assert_eq!(fetched.adapter, "zhihu hot");
    assert_eq!(fetched.kind, IssueKind::BadDescription);
    assert_eq!(fetched.title, "描述不准确");
    assert_eq!(fetched.status, IssueStatus::Open);

    cleanup(&path);
}

#[test]
fn test_issue_get_nonexistent_returns_none() {
    let path = temp_db();
    let store = IssueStore::new(path.clone()).unwrap();

    let result = store.get(99999).unwrap();
    assert!(result.is_none());

    cleanup(&path);
}

// ─────────────────────────────────────────────────────────────────────────────
// List / 过滤
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_issue_list_all() {
    let path = temp_db();
    let store = IssueStore::new(path.clone()).unwrap();

    store
        .add("adapter a", IssueKind::Broken, "t1", None)
        .unwrap();
    store
        .add("adapter b", IssueKind::Other, "t2", None)
        .unwrap();
    store
        .add("adapter a", IssueKind::BadDescription, "t3", None)
        .unwrap();

    let all = store.list(None, None, 100).unwrap();
    assert_eq!(all.len(), 3);

    cleanup(&path);
}

#[test]
fn test_issue_list_filter_by_adapter() {
    let path = temp_db();
    let store = IssueStore::new(path.clone()).unwrap();

    store
        .add("bilibili feed", IssueKind::Broken, "t1", None)
        .unwrap();
    store
        .add("zhihu hot", IssueKind::Broken, "t2", None)
        .unwrap();
    store
        .add("bilibili feed", IssueKind::Other, "t3", None)
        .unwrap();

    let filtered = store.list(Some("bilibili feed"), None, 100).unwrap();
    assert_eq!(filtered.len(), 2);
    assert!(filtered.iter().all(|i| i.adapter == "bilibili feed"));

    cleanup(&path);
}

#[test]
fn test_issue_list_filter_by_status_open() {
    let path = temp_db();
    let store = IssueStore::new(path.clone()).unwrap();

    let i1 = store
        .add("a", IssueKind::Broken, "open issue", None)
        .unwrap();
    let i2 = store
        .add("b", IssueKind::Broken, "will close", None)
        .unwrap();
    store.close(i2.id).unwrap();

    let open = store.list(None, Some(IssueStatus::Open), 100).unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].id, i1.id);

    cleanup(&path);
}

#[test]
fn test_issue_list_filter_by_status_closed() {
    let path = temp_db();
    let store = IssueStore::new(path.clone()).unwrap();

    store.add("a", IssueKind::Broken, "open", None).unwrap();
    let i2 = store.add("b", IssueKind::Broken, "closed", None).unwrap();
    store.close(i2.id).unwrap();

    let closed = store.list(None, Some(IssueStatus::Closed), 100).unwrap();
    assert_eq!(closed.len(), 1);
    assert_eq!(closed[0].id, i2.id);

    cleanup(&path);
}

#[test]
fn test_issue_list_respects_limit() {
    let path = temp_db();
    let store = IssueStore::new(path.clone()).unwrap();

    for i in 0..10 {
        store
            .add("a", IssueKind::Broken, &format!("issue {i}"), None)
            .unwrap();
    }

    let limited = store.list(None, None, 3).unwrap();
    assert_eq!(limited.len(), 3);

    cleanup(&path);
}

// ─────────────────────────────────────────────────────────────────────────────
// Close / Delete
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_issue_close_changes_status() {
    let path = temp_db();
    let store = IssueStore::new(path.clone()).unwrap();

    let issue = store.add("a", IssueKind::Broken, "broken", None).unwrap();
    assert_eq!(issue.status, IssueStatus::Open);

    let closed = store.close(issue.id).unwrap();
    assert!(closed, "close() should return true for open issue");

    let fetched = store.get(issue.id).unwrap().unwrap();
    assert_eq!(fetched.status, IssueStatus::Closed);

    cleanup(&path);
}

#[test]
fn test_issue_close_already_closed_returns_false() {
    let path = temp_db();
    let store = IssueStore::new(path.clone()).unwrap();

    let issue = store.add("a", IssueKind::Broken, "broken", None).unwrap();
    store.close(issue.id).unwrap();

    // 再次关闭 — 应该返回 false（没有行被更新）
    let result = store.close(issue.id).unwrap();
    assert!(
        !result,
        "closing an already-closed issue should return false"
    );

    cleanup(&path);
}

#[test]
fn test_issue_close_nonexistent_returns_false() {
    let path = temp_db();
    let store = IssueStore::new(path.clone()).unwrap();

    let result = store.close(99999).unwrap();
    assert!(!result);

    cleanup(&path);
}

#[test]
fn test_issue_delete_removes_issue() {
    let path = temp_db();
    let store = IssueStore::new(path.clone()).unwrap();

    let issue = store
        .add("a", IssueKind::Broken, "to delete", None)
        .unwrap();
    let deleted = store.delete(issue.id).unwrap();
    assert!(deleted);

    let fetched = store.get(issue.id).unwrap();
    assert!(fetched.is_none());

    cleanup(&path);
}

#[test]
fn test_issue_delete_nonexistent_returns_false() {
    let path = temp_db();
    let store = IssueStore::new(path.clone()).unwrap();

    let result = store.delete(99999).unwrap();
    assert!(!result);

    cleanup(&path);
}

#[test]
fn test_issue_delete_closed_issue() {
    let path = temp_db();
    let store = IssueStore::new(path.clone()).unwrap();

    let issue = store.add("a", IssueKind::Broken, "t", None).unwrap();
    store.close(issue.id).unwrap();
    let deleted = store.delete(issue.id).unwrap();
    assert!(deleted);

    let fetched = store.get(issue.id).unwrap();
    assert!(fetched.is_none());

    cleanup(&path);
}

// ─────────────────────────────────────────────────────────────────────────────
// Export
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_issue_export_all_returns_valid_json() {
    let path = temp_db();
    let store = IssueStore::new(path.clone()).unwrap();

    store.add("a", IssueKind::Broken, "t1", None).unwrap();
    store
        .add("b", IssueKind::Other, "t2", Some("body"))
        .unwrap();

    let json_str = store.export(None).unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(&json_str).expect("export should produce valid JSON");

    let arr = parsed.as_array().expect("export should be a JSON array");
    assert_eq!(arr.len(), 2);

    cleanup(&path);
}

#[test]
fn test_issue_export_filter_by_status() {
    let path = temp_db();
    let store = IssueStore::new(path.clone()).unwrap();

    store.add("a", IssueKind::Broken, "open", None).unwrap();
    let i2 = store.add("b", IssueKind::Other, "closed", None).unwrap();
    store.close(i2.id).unwrap();

    // 只导出 open 的
    let json_str = store.export(Some(IssueStatus::Open)).unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_str(&json_str).unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["status"], "open");

    // 只导出 closed 的
    let json_str = store.export(Some(IssueStatus::Closed)).unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_str(&json_str).unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["status"], "closed");

    cleanup(&path);
}

#[test]
fn test_issue_export_empty_store() {
    let path = temp_db();
    let store = IssueStore::new(path.clone()).unwrap();

    let json_str = store.export(None).unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_str(&json_str).unwrap();
    assert!(arr.is_empty());

    cleanup(&path);
}

// ─────────────────────────────────────────────────────────────────────────────
// IssueKind / IssueStatus 枚举转换
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_issue_kind_from_str() {
    assert_eq!(IssueKind::from("broken"), IssueKind::Broken);
    assert_eq!(
        IssueKind::from("bad_description"),
        IssueKind::BadDescription
    );
    assert_eq!(IssueKind::from("other"), IssueKind::Other);
    // 未知值 fallback 到 Other
    assert_eq!(IssueKind::from("unknown_kind"), IssueKind::Other);
    assert_eq!(IssueKind::from(""), IssueKind::Other);
}

#[test]
fn test_issue_status_from_str() {
    assert_eq!(IssueStatus::from("open"), IssueStatus::Open);
    assert_eq!(IssueStatus::from("closed"), IssueStatus::Closed);
    // 未知值 fallback 到 Open
    assert_eq!(IssueStatus::from("anything"), IssueStatus::Open);
}

#[test]
fn test_issue_kind_display() {
    assert_eq!(IssueKind::Broken.to_string(), "broken");
    assert_eq!(IssueKind::BadDescription.to_string(), "bad_description");
    assert_eq!(IssueKind::Other.to_string(), "other");
}

#[test]
fn test_issue_status_display() {
    assert_eq!(IssueStatus::Open.to_string(), "open");
    assert_eq!(IssueStatus::Closed.to_string(), "closed");
}

// ─────────────────────────────────────────────────────────────────────────────
// IDs 自增连续性
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_issue_ids_are_autoincrement() {
    let path = temp_db();
    let store = IssueStore::new(path.clone()).unwrap();

    let i1 = store.add("a", IssueKind::Broken, "t1", None).unwrap();
    let i2 = store.add("a", IssueKind::Broken, "t2", None).unwrap();
    let i3 = store.add("a", IssueKind::Broken, "t3", None).unwrap();

    assert!(i2.id > i1.id);
    assert!(i3.id > i2.id);

    cleanup(&path);
}

// ─────────────────────────────────────────────────────────────────────────────
// 数据库持久化
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_issue_data_persists_across_store_instances() {
    let path = temp_db();

    let id = {
        let store = IssueStore::new(path.clone()).unwrap();
        store
            .add("a", IssueKind::Broken, "persisted", None)
            .unwrap()
            .id
    };

    // 重新打开同一个数据库
    let store2 = IssueStore::new(path.clone()).unwrap();
    let fetched = store2.get(id).unwrap().unwrap();
    assert_eq!(fetched.title, "persisted");

    cleanup(&path);
}
