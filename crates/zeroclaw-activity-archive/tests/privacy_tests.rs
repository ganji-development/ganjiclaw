//! Tests for the privacy manager.

use crate::privacy::PrivacyManager;
use crate::schema::*;
use parking_lot::Mutex;
use rusqlite::Connection;
use tempfile::NamedTempFile;
use std::sync::Arc;

#[test]
fn test_privacy_manager_creation() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path();

    let conn = Connection::open(db_path).unwrap();
    init_schema(&conn).unwrap();

    let privacy_manager = PrivacyManager::new(Arc::new(Mutex::new(conn)));

    assert!(privacy_manager.initialize_default_rules().is_ok());
}

#[test]
fn test_privacy_manager_add_exclusion() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path();

    let conn = Connection::open(db_path).unwrap();
    init_schema(&conn).unwrap();

    let privacy_manager = PrivacyManager::new(Arc::new(Mutex::new(conn)));

    let result = privacy_manager.add_exclusion(
        PrivacyRuleType::ExcludePath,
        "**/test/**".to_string(),
        PrivacyAction::Exclude,
    );

    assert!(result.is_ok());
}

#[test]
fn test_privacy_manager_list_rules() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path();

    let conn = Connection::open(db_path).unwrap();
    init_schema(&conn).unwrap();

    let privacy_manager = PrivacyManager::new(Arc::new(Mutex::new(conn)));

    // Add a rule
    privacy_manager.add_exclusion(
        PrivacyRuleType::ExcludePath,
        "**/test/**".to_string(),
        PrivacyAction::Exclude,
    ).unwrap();

    // List rules
    let rules = privacy_manager.list_rules().unwrap();

    assert!(!rules.is_empty());
    assert!(rules.iter().any(|r| r.pattern == "**/test/**"));
}

#[test]
fn test_privacy_manager_remove_rule() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path();

    let conn = Connection::open(db_path).unwrap();
    init_schema(&conn).unwrap();

    let privacy_manager = PrivacyManager::new(Arc::new(Mutex::new(conn)));

    // Add a rule
    privacy_manager.add_exclusion(
        PrivacyRuleType::ExcludePath,
        "**/test/**".to_string(),
        PrivacyAction::Exclude,
    ).unwrap();

    // Get rule ID
    let rules = privacy_manager.list_rules().unwrap();
    let rule_id = rules[0].id.clone();

    // Remove rule
    let result = privacy_manager.remove_rule(&rule_id);

    assert!(result.is_ok());

    // Verify rule is removed
    let rules_after = privacy_manager.list_rules().unwrap();
    assert!(!rules_after.iter().any(|r| r.id == rule_id));
}

#[test]
fn test_privacy_manager_should_exclude_path() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path();

    let conn = Connection::open(db_path).unwrap();
    init_schema(&conn).unwrap();

    let privacy_manager = PrivacyManager::new(Arc::new(Mutex::new(conn)));

    // Add exclusion rule
    privacy_manager.add_exclusion(
        PrivacyRuleType::ExcludePath,
        "**/passwords/**".to_string(),
        PrivacyAction::Exclude,
    ).unwrap();

    // Create event with sensitive path
    let mut event = Event::new("file_activity".to_string(), EventType::FileCreate);
    event.path = Some("/home/user/passwords/secret.txt".to_string());

    // Should be excluded
    assert!(privacy_manager.should_exclude(&event));
}

#[test]
fn test_privacy_manager_should_exclude_title() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path();

    let conn = Connection::open(db_path).unwrap();
    init_schema(&conn).unwrap();

    let privacy_manager = PrivacyManager::new(Arc::new(Mutex::new(conn)));

    // Add exclusion rule
    privacy_manager.add_exclusion(
        PrivacyRuleType::ExcludeTitle,
        "*password*".to_string(),
        PrivacyAction::Exclude,
    ).unwrap();

    // Create event with sensitive title
    let mut event = Event::new("window_focus".to_string(), EventType::WindowFocus);
    event.title = Some("Enter Password".to_string());

    // Should be excluded
    assert!(privacy_manager.should_exclude(&event));
}

#[test]
fn test_privacy_manager_should_exclude_domain() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path();

    let conn = Connection::open(db_path).unwrap();
    init_schema(&conn).unwrap();

    let privacy_manager = PrivacyManager::new(Arc::new(Mutex::new(conn)));

    // Add exclusion rule
    privacy_manager.add_exclusion(
        PrivacyRuleType::ExcludeDomain,
        "*.bank.com".to_string(),
        PrivacyAction::Exclude,
    ).unwrap();

    // Create event with sensitive domain
    let mut event = Event::new("browser_visit".to_string(), EventType::BrowserVisit);
    event.details = serde_json::json!({
        "url": "https://www.mybank.com/login"
    });

    // Should be excluded
    assert!(privacy_manager.should_exclude(&event));
}

#[test]
fn test_privacy_manager_does_not_exclude_safe_events() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path();

    let conn = Connection::open(db_path).unwrap();
    init_schema(&conn).unwrap();

    let privacy_manager = PrivacyManager::new(Arc::new(Mutex::new(conn)));

    // Create safe event
    let mut event = Event::new("window_focus".to_string(), EventType::WindowFocus);
    event.title = Some("Safe Document".to_string());
    event.path = Some("/home/user/documents/work.txt".to_string());

    // Should not be excluded
    assert!(!privacy_manager.should_exclude(&event));
}

#[test]
fn test_privacy_manager_redact() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path();

    let conn = Connection::open(db_path).unwrap();
    init_schema(&conn).unwrap();

    let privacy_manager = PrivacyManager::new(Arc::new(Mutex::new(conn)));

    // Add redaction rule
    privacy_manager.add_exclusion(
        PrivacyRuleType::Redaction,
        "*secret*".to_string(),
        PrivacyAction::Redact,
    ).unwrap();

    // Create event with sensitive title
    let mut event = Event::new("window_focus".to_string(), EventType::WindowFocus);
    event.title = Some("My Secret Document".to_string());

    // Apply redaction
    privacy_manager.redact(&mut event);

    // Title should be redacted
    assert_eq!(event.title, Some("[REDACTED]".to_string()));
}

#[test]
fn test_privacy_manager_hash() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path();

    let conn = Connection::open(db_path).unwrap();
    init_schema(&conn).unwrap();

    let privacy_manager = PrivacyManager::new(Arc::new(Mutex::new(conn)));

    // Add hashing rule
    privacy_manager.add_exclusion(
        PrivacyRuleType::Redaction,
        "*token*".to_string(),
        PrivacyAction::Hash,
    ).unwrap();

    // Create event with sensitive title
    let mut event = Event::new("window_focus".to_string(), EventType::WindowFocus);
    let original_title = "My Token Value".to_string();
    event.title = Some(original_title.clone());

    // Apply hashing
    privacy_manager.redact(&mut event);

    // Title should be hashed (not equal to original)
    assert_ne!(event.title, Some(original_title));
    // Hashed value should be hex string
    let hashed = event.title.unwrap();
    assert!(hashed.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn test_privacy_manager_default_rules() {
    let default_rules = PrivacyManager::default_rules();

    // Should have default rules for common sensitive patterns
    assert!(!default_rules.is_empty());

    // Check for password exclusion
    assert!(default_rules.iter().any(|r| {
        r.rule_type == PrivacyRuleType::ExcludePath
            && r.pattern.contains("passwords")
    }));

    // Check for banking exclusion
    assert!(default_rules.iter().any(|r| {
        r.rule_type == PrivacyRuleType::ExcludePath
            && r.pattern.contains("banking")
    }));

    // Check for password title exclusion
    assert!(default_rules.iter().any(|r| {
        r.rule_type == PrivacyRuleType::ExcludeTitle
            && r.pattern.contains("password")
    }));

    // Check for bank domain exclusion
    assert!(default_rules.iter().any(|r| {
        r.rule_type == PrivacyRuleType::ExcludeDomain
            && r.pattern.contains("bank")
    }));
}

#[test]
fn test_privacy_manager_initialize_default_rules() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path();

    let conn = Connection::open(db_path).unwrap();
    init_schema(&conn).unwrap();

    let privacy_manager = PrivacyManager::new(Arc::new(Mutex::new(conn)));

    // Initialize default rules
    assert!(privacy_manager.initialize_default_rules().is_ok());

    // Verify rules were added
    let rules = privacy_manager.list_rules().unwrap();
    assert!(!rules.is_empty());
}

#[test]
fn test_privacy_manager_matches_pattern() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path();

    let conn = Connection::open(db_path).unwrap();
    init_schema(&conn).unwrap();

    let privacy_manager = PrivacyManager::new(Arc::new(Mutex::new(conn)));

    // Test glob patterns
    assert!(privacy_manager.matches_pattern("/home/user/passwords/secret.txt", "**/passwords/**"));
    assert!(privacy_manager.matches_pattern("/home/user/passwords/secret.txt", "**/passwords/*"));
    assert!(!privacy_manager.matches_pattern("/home/user/documents/file.txt", "**/passwords/**"));

    // Test wildcard
    assert!(privacy_manager.matches_pattern("test.txt", "*.txt"));
    assert!(!privacy_manager.matches_pattern("test.txt", "*.md"));

    // Test exact match
    assert!(privacy_manager.matches_pattern("exact_match", "exact_match"));
    assert!(!privacy_manager.matches_pattern("exact_match", "different_match"));
}

#[test]
fn test_privacy_manager_hash_value() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path();

    let conn = Connection::open(db_path).unwrap();
    init_schema(&conn).unwrap();

    let privacy_manager = PrivacyManager::new(Arc::new(Mutex::new(conn)));

    let value1 = "test_value";
    let value2 = "test_value";
    let value3 = "different_value";

    let hash1 = privacy_manager.hash_value(value1);
    let hash2 = privacy_manager.hash_value(value2);
    let hash3 = privacy_manager.hash_value(value3);

    // Same values should produce same hash
    assert_eq!(hash1, hash2);

    // Different values should produce different hash
    assert_ne!(hash1, hash3);

    // Hash should be hex string
    assert!(hash1.chars().all(|c| c.is_ascii_hexdigit()));
}
