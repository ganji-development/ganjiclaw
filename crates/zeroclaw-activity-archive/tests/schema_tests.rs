//! Tests for the activity archive database schema.

use zeroclaw_activity_archive::schema::*;
use rusqlite::Connection;
use tempfile::NamedTempFile;

#[test]
fn test_init_schema() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path();

    let conn = Connection::open(db_path).unwrap();
    init_schema(&conn).unwrap();

    // Verify tables were created
    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table'")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();

    assert!(tables.contains(&"raw_events".to_string()));
    assert!(tables.contains(&"events".to_string()));
    assert!(tables.contains(&"sessions".to_string()));
    assert!(tables.contains(&"entities".to_string()));
    assert!(tables.contains(&"event_entity_map".to_string()));
    assert!(tables.contains(&"summaries".to_string()));
    assert!(tables.contains(&"artifacts".to_string()));
    assert!(tables.contains(&"notion_sync_queue".to_string()));
    assert!(tables.contains(&"ingestion_offsets".to_string()));
    assert!(tables.contains(&"privacy_rules".to_string()));
}

#[test]
fn test_open_connection() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path();

    let conn = open_connection(db_path, None).unwrap();
    init_schema(&conn).unwrap();

    // Verify connection is working
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn test_event_creation() {
    let event = Event::new("test_source".to_string(), EventType::WindowFocus);

    assert_eq!(event.source, "test_source");
    assert_eq!(event.event_type, EventType::WindowFocus);
    assert!(event.id.len() > 0);
    assert!(event.ts_utc <= chrono::Utc::now());
}

#[test]
fn test_event_hash_generation() {
    let mut event1 = Event::new("test_source".to_string(), EventType::WindowFocus);
    event1.app = Some("TestApp".to_string());
    event1.title = Some("Test Title".to_string());

    let mut event2 = Event::new("test_source".to_string(), EventType::WindowFocus);
    event2.app = Some("TestApp".to_string());
    event2.title = Some("Test Title".to_string());

    let hash1 = event1.generate_hash();
    let hash2 = event2.generate_hash();

    assert_eq!(hash1, hash2);

    // Different content should produce different hash
    event2.title = Some("Different Title".to_string());
    let hash3 = event2.generate_hash();
    assert_ne!(hash1, hash3);
}

#[test]
fn test_session_creation() {
    let start_time = chrono::Utc::now();
    let session = Session::new(start_time);

    assert_eq!(session.start_ts_utc, start_time);
    assert!(session.end_ts_utc.is_none());
    assert_eq!(session.label, SessionLabel::Unknown);
    assert_eq!(session.event_count, 0);
}

#[test]
fn test_entity_creation() {
    let entity = Entity::new(EntityType::App, "TestApp".to_string());

    assert_eq!(entity.entity_type, EntityType::App);
    assert_eq!(entity.name, "TestApp");
    assert_eq!(entity.occurrence_count, 1);
}

#[test]
fn test_summary_creation() {
    let start = chrono::Utc::now();
    let end = start + chrono::Duration::hours(1);
    let summary = Summary::new(SummaryType::Hourly, start, end);

    assert_eq!(summary.summary_type, SummaryType::Hourly);
    assert_eq!(summary.period_start, start);
    assert_eq!(summary.period_end, end);
    assert!(summary.content.is_empty());
}

#[test]
fn test_privacy_rule_creation() {
    let rule = PrivacyRule::new(
        PrivacyRuleType::ExcludePath,
        "**/passwords/**".to_string(),
        PrivacyAction::Exclude,
    );

    assert_eq!(rule.rule_type, PrivacyRuleType::ExcludePath);
    assert_eq!(rule.pattern, "**/passwords/**");
    assert_eq!(rule.action, PrivacyAction::Exclude);
}

#[test]
fn test_event_type_conversions() {
    assert_eq!(EventType::WindowFocus.as_str(), "window_focus");
    assert_eq!(EventType::ProcessStart.as_str(), "process_start");
    assert_eq!(EventType::BrowserVisit.as_str(), "browser_visit");

    assert_eq!(EventType::from_str("window_focus"), Some(EventType::WindowFocus));
    assert_eq!(EventType::from_str("invalid"), None);
}

#[test]
fn test_session_label_conversions() {
    assert_eq!(SessionLabel::Coding.as_str(), "coding");
    assert_eq!(SessionLabel::Research.as_str(), "research");

    assert_eq!(SessionLabel::from_str("coding"), Some(SessionLabel::Coding));
    assert_eq!(SessionLabel::from_str("invalid"), None);
}

#[test]
fn test_entity_type_conversions() {
    assert_eq!(EntityType::App.as_str(), "app");
    assert_eq!(EntityType::Project.as_str(), "project");

    assert_eq!(EntityType::from_str("app"), Some(EntityType::App));
    assert_eq!(EntityType::from_str("invalid"), None);
}

#[test]
fn test_summary_type_conversions() {
    assert_eq!(SummaryType::Hourly.as_str(), "hourly");
    assert_eq!(SummaryType::Daily.as_str(), "daily");

    assert_eq!(SummaryType::from_str("hourly"), Some(SummaryType::Hourly));
    assert_eq!(SummaryType::from_str("invalid"), None);
}

#[test]
fn test_artifact_type_conversions() {
    assert_eq!(ArtifactType::Screenshot.as_str(), "screenshot");
    assert_eq!(ArtifactType::FileExport.as_str(), "file_export");

    assert_eq!(ArtifactType::from_str("screenshot"), Some(ArtifactType::Screenshot));
    assert_eq!(ArtifactType::from_str("invalid"), None);
}

#[test]
fn test_notion_sync_type_conversions() {
    assert_eq!(NotionSyncType::DailyLog.as_str(), "daily_log");
    assert_eq!(NotionSyncType::Session.as_str(), "session");

    assert_eq!(NotionSyncType::from_str("daily_log"), Some(NotionSyncType::DailyLog));
    assert_eq!(NotionSyncType::from_str("invalid"), None);
}

#[test]
fn test_sync_status_conversions() {
    assert_eq!(SyncStatus::Pending.as_str(), "pending");
    assert_eq!(SyncStatus::Synced.as_str(), "synced");

    assert_eq!(SyncStatus::from_str("pending"), Some(SyncStatus::Pending));
    assert_eq!(SyncStatus::from_str("invalid"), None);
}

#[test]
fn test_privacy_rule_type_conversions() {
    assert_eq!(PrivacyRuleType::ExcludePath.as_str(), "exclude_path");
    assert_eq!(PrivacyRuleType::ExcludeTitle.as_str(), "exclude_title");

    assert_eq!(PrivacyRuleType::from_str("exclude_path"), Some(PrivacyRuleType::ExcludePath));
    assert_eq!(PrivacyRuleType::from_str("invalid"), None);
}

#[test]
fn test_privacy_action_conversions() {
    assert_eq!(PrivacyAction::Exclude.as_str(), "exclude");
    assert_eq!(PrivacyAction::Redact.as_str(), "redact");

    assert_eq!(PrivacyAction::from_str("exclude"), Some(PrivacyAction::Exclude));
    assert_eq!(PrivacyAction::from_str("invalid"), None);
}

#[test]
fn test_raw_event_creation() {
    let payload = serde_json::json!({
        "window_title": "Test Window",
        "process_id": 1234,
    });

    let event = RawEvent::new("window_focus".to_string(), payload.clone());

    assert_eq!(event.source, "window_focus");
    assert_eq!(event.payload, payload);
    assert!(event.id.len() > 0);
    assert!(event.timestamp <= chrono::Utc::now());
}
