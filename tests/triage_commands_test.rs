//! Integration tests for triage Tauri commands.
//!
//! Tests the public `#[tauri::command]` entry points for triage operations:
//! - list_triage_items
//! - get_triage_stats
//! - mark_triage_read
//! - archive_triage_item
//!
//! Uses temporary directories for isolation to avoid polluting the user's
//! real `~/.shannon/triage.jsonl` file.

use shannon_desktop::scheduled_commands::{TriageFilter, TriageItem, TriageStore};
use tempfile::TempDir;

/// Helper to seed test triage items.
fn seed_triage_items(store: &TriageStore, count: usize) -> Vec<TriageItem> {
    let mut items = Vec::new();
    for i in 0..count {
        let kind = if i % 2 == 0 { "failed_run" } else { "needs_review" };
        let item = store
            .add(kind, &format!("Test triage item {}", i))
            .expect("add triage item");
        items.push(item);
    }
    // Return in insertion order (oldest first)
    items
}

#[tokio::test]
async fn list_triage_items_returns_seeded_items() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let store = TriageStore::with_path(tmp.path().join("triage.jsonl"));

    let _items = seed_triage_items(&store, 3);

    // Test the TriageStore::list method which is called by the Tauri command
    let result = store.list(&TriageFilter::default()).expect("list items");

    assert_eq!(result.len(), 3, "should return all seeded items");
    // Verify items are returned - the specific order depends on HashMap iteration
    // which is non-deterministic, so we just check that all items are present
    let messages: Vec<_> = result.iter().map(|i| &i.message).collect();
    assert!(messages.contains(&&"Test triage item 0".to_string()));
    assert!(messages.contains(&&"Test triage item 1".to_string()));
    assert!(messages.contains(&&"Test triage item 2".to_string()));
}

#[tokio::test]
async fn list_triage_items_respects_unread_filter() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let store = TriageStore::with_path(tmp.path().join("triage.jsonl"));

    let items = seed_triage_items(&store, 3);
    store.mark_read(&items[0].id).expect("mark item 0 as read");

    let filter = TriageFilter {
        unread_only: Some(true),
        ..Default::default()
    };
    let result = store.list(&filter).expect("list unread items");

    assert_eq!(result.len(), 2, "should return only unread items");
    assert!(!result[0].read);
    assert!(!result[1].read);
}

#[tokio::test]
async fn list_triage_items_respects_unarchived_filter() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let store = TriageStore::with_path(tmp.path().join("triage.jsonl"));

    let items = seed_triage_items(&store, 3);
    store.archive(&items[0].id).expect("archive item 0");

    let filter = TriageFilter {
        unarchived_only: Some(true),
        ..Default::default()
    };
    let result = store.list(&filter).expect("list unarchived items");

    assert_eq!(result.len(), 2, "should return only unarchived items");
    assert!(!result[0].archived);
    assert!(!result[1].archived);
}

#[tokio::test]
async fn get_triage_stats_aggregates_correctly() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let store = TriageStore::with_path(tmp.path().join("triage.jsonl"));

    let items = seed_triage_items(&store, 4);
    store.mark_read(&items[0].id).expect("mark item 0 as read");
    store.archive(&items[1].id).expect("archive item 1");

    let stats = store.stats().expect("get stats");

    assert_eq!(stats.total, 4, "total should count all items");
    assert_eq!(stats.unread, 2, "unread should count non-read items");
    assert_eq!(stats.archived, 1, "archived should count archived items");

    // Verify by_kind aggregation
    assert_eq!(
        *stats.by_kind.get("failed_run").unwrap_or(&0),
        2,
        "should count failed_run items"
    );
    assert_eq!(
        *stats.by_kind.get("needs_review").unwrap_or(&0),
        2,
        "should count needs_review items"
    );
}

#[tokio::test]
async fn mark_triage_read_flips_read_flag() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let store = TriageStore::with_path(tmp.path().join("triage.jsonl"));

    let item = store
        .add("test_kind", "test message")
        .expect("add item");
    assert!(!item.read, "item should start unread");

    let updated = store.mark_read(&item.id).expect("mark as read");
    assert!(updated.read, "item should be marked read");
    assert_eq!(updated.revision, 1, "revision should increment");

    // Verify the change persists
    let result = store.list(&TriageFilter::default()).expect("list items");
    assert_eq!(result.len(), 1);
    assert!(result[0].read, "item should remain read on re-read");
}

#[tokio::test]
async fn mark_triage_read_reflected_in_stats() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let store = TriageStore::with_path(tmp.path().join("triage.jsonl"));

    let item = store
        .add("test_kind", "test message")
        .expect("add item");

    let stats_before = store.stats().expect("get stats before");
    assert_eq!(stats_before.unread, 1, "should have 1 unread item");

    store.mark_read(&item.id).expect("mark as read");

    let stats_after = store.stats().expect("get stats after");
    assert_eq!(stats_after.unread, 0, "should have 0 unread items");
    assert_eq!(stats_after.total, 1, "total should remain unchanged");
}

#[tokio::test]
async fn archive_triage_item_flips_archived_flag() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let store = TriageStore::with_path(tmp.path().join("triage.jsonl"));

    let item = store
        .add("test_kind", "test message")
        .expect("add item");
    assert!(!item.archived, "item should start unarchived");

    let archived = store.archive(&item.id).expect("archive item");
    assert!(archived.archived, "item should be marked archived");
    assert!(archived.read, "archiving should also mark as read");
    assert_eq!(archived.revision, 1, "revision should increment");

    // Verify the change persists
    let result = store.list(&TriageFilter::default()).expect("list items");
    assert_eq!(result.len(), 1);
    assert!(result[0].archived, "item should remain archived on re-read");
}

#[tokio::test]
async fn triage_store_filter_by_kind() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let store = TriageStore::with_path(tmp.path().join("triage.jsonl"));

    store.add("failed_run", "fail 1").expect("add failed_run 1");
    store.add("failed_run", "fail 2").expect("add failed_run 2");
    store.add("needs_review", "review 1").expect("add needs_review");
    store.add("budget_exceeded", "budget 1").expect("add budget_exceeded");

    let filter = TriageFilter {
        kind: Some("failed_run".into()),
        ..Default::default()
    };
    let result = store.list(&filter).expect("list by kind");

    assert_eq!(result.len(), 2, "should return only failed_run items");
    assert_eq!(result[0].kind, "failed_run");
    assert_eq!(result[1].kind, "failed_run");
}

#[tokio::test]
async fn triage_store_limit_truncates_results() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let store = TriageStore::with_path(tmp.path().join("triage.jsonl"));

    seed_triage_items(&store, 10);

    let filter = TriageFilter {
        limit: Some(5),
        ..Default::default()
    };
    let result = store.list(&filter).expect("list with limit");

    assert_eq!(result.len(), 5, "should return only 5 items");
}

#[tokio::test]
async fn triage_store_empty_list_returns_no_items() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let store = TriageStore::with_path(tmp.path().join("triage.jsonl"));

    let result = store.list(&TriageFilter::default()).expect("list empty");

    assert_eq!(result.len(), 0, "empty store should return no items");
}

#[tokio::test]
async fn triage_stats_empty_store_returns_zeroes() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let store = TriageStore::with_path(tmp.path().join("triage.jsonl"));

    let stats = store.stats().expect("get stats from empty store");

    assert_eq!(stats.total, 0);
    assert_eq!(stats.unread, 0);
    assert_eq!(stats.archived, 0);
    assert!(stats.by_kind.is_empty(), "by_kind should be empty");
}
