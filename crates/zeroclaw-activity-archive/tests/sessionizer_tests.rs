//! Tests for the sessionizer.

use crate::sessionizer::Sessionizer;
use crate::schema::*;
use parking_lot::Mutex;
use rusqlite::Connection;
use tempfile::NamedTempFile;
use std::sync::Arc;
use chrono::{Duration, Utc};

#[test]
fn test_sessionizer_creation() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path();

    let conn = Connection::open(db_path).unwrap();
    init_schema(&conn).unwrap();

    let sessionizer = Sessionizer::new(
        Arc::new(Mutex::new(conn)),
        30, // idle_timeout_minutes
        15, // context_switch_threshold_minutes
    );

    assert_eq!(sessionizer.idle_timeout_minutes, 30);
    assert_eq!(sessionizer.context_switch_threshold_minutes, 15);
}

#[test]
fn test_sessionizer_update_sessions_empty() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path();

    let conn = Connection::open(db_path).unwrap();
    init_schema(&conn).unwrap();

    let sessionizer = Sessionizer::new(
        Arc::new(Mutex::new(conn)),
        30,
        15,
    );

    // Should succeed even with no events
    assert!(sessionizer.update_sessions().is_ok());
}

#[test]
fn test_sessionizer_group_events_into_sessions() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path();

    let conn = Connection::open(db_path).unwrap();
    init_schema(&conn).unwrap();

    let sessionizer = Sessionizer::new(
        Arc::new(Mutex::new(conn)),
        30,
        15,
    );

    let now = Utc::now();

    // Create events with different time gaps
    let events = vec![
        {
            let mut e = Event::new("window_focus".to_string(), EventType::WindowFocus);
            e.ts_utc = now;
            e.app = Some("App1".to_string());
            e
        },
        {
            let mut e = Event::new("window_focus".to_string(), EventType::WindowFocus);
            e.ts_utc = now + Duration::minutes(5);
            e.app = Some("App1".to_string());
            e
        },
        {
            let mut e = Event::new("window_focus".to_string(), EventType::WindowFocus);
            e.ts_utc = now + Duration::minutes(10);
            e.app = Some("App2".to_string()); // Context switch
            e
        },
    ];

    let sessions = sessionizer.group_events_into_sessions(&events);

    // Should create at least one session
    assert!(!sessions.is_empty());

    // First session should have events from App1
    let first_session = &sessions[0];
    assert_eq!(first_session.start_ts_utc, now);
    assert!(first_session.event_count >= 2);
}

#[test]
fn test_sessionizer_idle_timeout() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path();

    let conn = Connection::open(db_path).unwrap();
    init_schema(&conn).unwrap();

    let sessionizer = Sessionizer::new(
        Arc::new(Mutex::new(conn)),
        30, // 30 minute idle timeout
        15,
    );

    let now = Utc::now();

    // Create events with large time gap (exceeds idle timeout)
    let events = vec![
        {
            let mut e = Event::new("window_focus".to_string(), EventType::WindowFocus);
            e.ts_utc = now;
            e.app = Some("App1".to_string());
            e
        },
        {
            let mut e = Event::new("window_focus".to_string(), EventType::WindowFocus);
            e.ts_utc = now + Duration::minutes(45); // 45 minute gap
            e.app = Some("App1".to_string());
            e
        },
    ];

    let sessions = sessionizer.group_events_into_sessions(&events);

    // Should create two sessions due to idle timeout
    assert_eq!(sessions.len(), 2);
}

#[test]
fn test_sessionizer_context_switch() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path();

    let conn = Connection::open(db_path).unwrap();
    init_schema(&conn).unwrap();

    let sessionizer = Sessionizer::new(
        Arc::new(Mutex::new(conn)),
        30,
        15, // 15 minute context switch threshold
    );

    let now = Utc::now();

    // Create events with context switch (different app within threshold)
    let events = vec![
        {
            let mut e = Event::new("window_focus".to_string(), EventType::WindowFocus);
            e.ts_utc = now;
            e.app = Some("App1".to_string());
            e
        },
        {
            let mut e = Event::new("window_focus".to_string(), EventType::WindowFocus);
            e.ts_utc = now + Duration::minutes(10);
            e.app = Some("App2".to_string()); // Context switch
            e
        },
    ];

    let sessions = sessionizer.group_events_into_sessions(&events);

    // Should create two sessions due to context switch
    assert_eq!(sessions.len(), 2);
}

#[test]
fn test_sessionizer_infer_session_label() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path();

    let conn = Connection::open(db_path).unwrap();
    init_schema(&conn).unwrap();

    let sessionizer = Sessionizer::new(
        Arc::new(Mutex::new(conn)),
        30,
        15,
    );

    let now = Utc::now();

    // Create session with project
    let mut session = Session::new(now);
    session.project_key = Some("myproject".to_string());

    let label = sessionizer.infer_session_label(&session);

    // Should infer coding label for project
    assert_eq!(label, SessionLabel::Coding);
}

#[test]
fn test_sessionizer_extract_context() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path();

    let conn = Connection::open(db_path).unwrap();
    init_schema(&conn).unwrap();

    let sessionizer = Sessionizer::new(
        Arc::new(Mutex::new(conn)),
        30,
        15,
    );

    let mut event = Event::new("window_focus".to_string(), EventType::WindowFocus);
    event.app = Some("VSCode".to_string());
    event.project_key = Some("myproject".to_string());

    let context = sessionizer.extract_context(&event);

    assert_eq!(context, "VSCode:myproject");
}

#[test]
fn test_sessionizer_get_active_session() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path();

    let conn = Connection::open(db_path).unwrap();
    init_schema(&conn).unwrap();

    let sessionizer = Sessionizer::new(
        Arc::new(Mutex::new(conn)),
        30,
        15,
    );

    // Initially no active session
    let active = sessionizer.get_active_session().unwrap();
    assert!(active.is_none());
}

#[test]
fn test_sessionizer_end_active_session() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path();

    let conn = Connection::open(db_path).unwrap();
    init_schema(&conn).unwrap();

    let sessionizer = Sessionizer::new(
        Arc::new(Mutex::new(conn)),
        30,
        15,
    );

    // Should succeed even if no active session
    assert!(sessionizer.end_active_session().is_ok());
}

#[test]
fn test_session_label_conversions() {
    assert_eq!(SessionLabel::Coding.as_str(), "coding");
    assert_eq!(SessionLabel::Research.as_str(), "research");
    assert_eq!(SessionLabel::Admin.as_str(), "admin");
    assert_eq!(SessionLabel::Messaging.as_str(), "messaging");
    assert_eq!(SessionLabel::Music.as_str(), "music");
    assert_eq!(SessionLabel::Writing.as_str(), "writing");
    assert_eq!(SessionLabel::Design.as_str(), "design");
    assert_eq!(SessionLabel::Unknown.as_str(), "unknown");

    assert_eq!(SessionLabel::from_str("coding"), Some(SessionLabel::Coding));
    assert_eq!(SessionLabel::from_str("invalid"), None);
}
