//! Tests for the collector framework.

use crate::collector::{Collector, OffsetTracker};
use crate::collectors::{WindowFocusCollector, ProcessLaunchCollector, BrowserHistoryCollector, ShellActivityCollector, FileActivityCollector};
use crate::schema::RawEvent;
use tempfile::NamedTempFile;
use std::path::PathBuf;

#[test]
fn test_offset_tracker_creation() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let tracker = OffsetTracker::new("test_collector".to_string(), db_path);

    assert_eq!(tracker.collector_name, "test_collector");
}

#[test]
fn test_offset_tracker_get_offset_initial() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let tracker = OffsetTracker::new("test_collector".to_string(), db_path);

    // Initialize database
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    crate::schema::init_schema(&conn).unwrap();

    // Get initial offset (should be None)
    let offset = tokio::runtime::Runtime::new().unwrap().block_on(async {
        tracker.get_offset().await
    });

    assert!(offset.is_none());
}

#[test]
fn test_offset_tracker_save_and_get() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let tracker = OffsetTracker::new("test_collector".to_string(), db_path);

    // Initialize database
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    crate::schema::init_schema(&conn).unwrap();

    let rt = tokio::runtime::Runtime::new().unwrap();

    // Save offset
    rt.block_on(async {
        tracker.save_offset("test_offset_123".to_string()).await
    }).unwrap();

    // Get offset
    let offset = rt.block_on(async {
        tracker.get_offset().await
    }).unwrap();

    assert_eq!(offset, Some("test_offset_123".to_string()));
}

#[test]
fn test_raw_event_creation() {
    let payload = serde_json::json!({
        "test_field": "test_value",
    });

    let event = RawEvent::new("test_source".to_string(), payload.clone());

    assert_eq!(event.source, "test_source");
    assert_eq!(event.payload, payload);
    assert!(!event.id.is_empty());
}

#[test]
fn test_window_focus_collector_creation() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let collector = crate::collectors::WindowFocusCollector::new(db_path, 2);

    assert_eq!(collector.name(), "window_focus");
}

#[test]
fn test_process_launch_collector_creation() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let collector = crate::collectors::ProcessLaunchCollector::new(db_path);

    assert_eq!(collector.name(), "process_launch");
}

#[test]
fn test_browser_history_collector_creation() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let collector = crate::collectors::BrowserHistoryCollector::new(db_path);

    assert_eq!(collector.name(), "browser_history");
}

#[test]
fn test_shell_activity_collector_creation() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let collector = crate::collectors::ShellActivityCollector::new(db_path);

    assert_eq!(collector.name(), "shell_activity");
}

#[test]
fn test_file_activity_collector_creation() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let folders = vec![
        temp_file.path().to_path_buf(),
        PathBuf::from("/test/folder"),
    ];

    let collector = crate::collectors::FileActivityCollector::new(db_path, folders);

    assert_eq!(collector.name(), "file_activity");
}

#[test]
fn test_collector_trait_methods() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let collector = crate::collectors::WindowFocusCollector::new(db_path, 2);

    // Test trait methods
    assert_eq!(collector.name(), "window_focus");

    let rt = tokio::runtime::Runtime::new().unwrap();

    // Test get_offset
    let offset = rt.block_on(async {
        collector.get_offset().await
    });
    assert!(offset.is_ok());

    // Test stop
    let stop_result = rt.block_on(async {
        collector.stop().await
    });
    assert!(stop_result.is_ok());
}

#[test]
fn test_multiple_collectors() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let collectors: Vec<Box<dyn Collector>> = vec![
        Box::new(crate::collectors::WindowFocusCollector::new(db_path.clone(), 2)),
        Box::new(crate::collectors::ProcessLaunchCollector::new(db_path.clone())),
        Box::new(crate::collectors::BrowserHistoryCollector::new(db_path.clone())),
        Box::new(crate::collectors::ShellActivityCollector::new(db_path.clone())),
    ];

    assert_eq!(collectors.len(), 4);

    for collector in collectors {
        assert!(!collector.name().is_empty());
    }
}
