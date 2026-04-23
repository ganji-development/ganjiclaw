//! Integration tests for the activity archive system.

use crate::runtime::{ActivityArchiveRuntime, ActivityArchiveConfig, CollectorConfig, SessionizerConfig, SummarizerConfig, NotionSyncConfig, PrivacyConfig};
use tempfile::TempDir;
use std::path::PathBuf;

#[test]
fn test_runtime_creation() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_dir = temp_dir.path().to_path_buf();

    let config = ActivityArchiveConfig {
        enabled: true,
        database_path: None,
        collectors: CollectorConfig {
            window_focus: false,
            process_launch: false,
            browser_history: false,
            shell_activity: false,
            file_activity: false,
            file_activity_folders: vec![],
            poll_interval_seconds: 2,
        },
        sessionizer: SessionizerConfig {
            idle_timeout_minutes: 30,
            context_switch_threshold_minutes: 15,
        },
        summarizer: SummarizerConfig {
            enabled: true,
            hourly_summary_enabled: true,
            daily_log_enabled: true,
            project_summary_enabled: true,
        },
        notion_sync: NotionSyncConfig {
            enabled: false,
            api_key: String::new(),
            daily_logs_database_id: String::new(),
            sessions_database_id: String::new(),
            projects_database_id: String::new(),
            sync_interval_minutes: 5,
        },
        privacy: PrivacyConfig {
            exclude_paths: vec![],
            exclude_titles: vec![],
            exclude_domains: vec![],
            redact_clipboard: true,
            clipboard_whitelist: vec![],
        },
    };

    let runtime = ActivityArchiveRuntime::new(config, &workspace_dir);

    assert!(runtime.is_ok());

    let runtime = runtime.unwrap();
    let status = runtime.status();

    assert!(status.enabled);
    assert_eq!(status.collectors_running, 0);
}

#[test]
fn test_runtime_with_collectors() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_dir = temp_dir.path().to_path_buf();

    let config = ActivityArchiveConfig {
        enabled: true,
        database_path: None,
        collectors: CollectorConfig {
            window_focus: true,
            process_launch: true,
            browser_history: true,
            shell_activity: true,
            file_activity: false,
            file_activity_folders: vec![],
            poll_interval_seconds: 2,
        },
        sessionizer: SessionizerConfig {
            idle_timeout_minutes: 30,
            context_switch_threshold_minutes: 15,
        },
        summarizer: SummarizerConfig {
            enabled: true,
            hourly_summary_enabled: true,
            daily_log_enabled: true,
            project_summary_enabled: true,
        },
        notion_sync: NotionSyncConfig {
            enabled: false,
            api_key: String::new(),
            daily_logs_database_id: String::new(),
            sessions_database_id: String::new(),
            projects_database_id: String::new(),
            sync_interval_minutes: 5,
        },
        privacy: PrivacyConfig {
            exclude_paths: vec![],
            exclude_titles: vec![],
            exclude_domains: vec![],
            redact_clipboard: true,
            clipboard_whitelist: vec![],
        },
    };

    let runtime = ActivityArchiveRuntime::new(config, &workspace_dir);

    assert!(runtime.is_ok());

    let runtime = runtime.unwrap();
    let status = runtime.status();

    assert_eq!(status.collectors_running, 3); // window_focus, process_launch, browser_history, shell_activity
}

#[test]
fn test_runtime_shutdown() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_dir = temp_dir.path().to_path_buf();

    let config = ActivityArchiveConfig {
        enabled: true,
        database_path: None,
        collectors: CollectorConfig {
            window_focus: false,
            process_launch: false,
            browser_history: false,
            shell_activity: false,
            file_activity: false,
            file_activity_folders: vec![],
            poll_interval_seconds: 2,
        },
        sessionizer: SessionizerConfig {
            idle_timeout_minutes: 30,
            context_switch_threshold_minutes: 15,
        },
        summarizer: SummarizerConfig {
            enabled: true,
            hourly_summary_enabled: true,
            daily_log_enabled: true,
            project_summary_enabled: true,
        },
        notion_sync: NotionSyncConfig {
            enabled: false,
            api_key: String::new(),
            daily_logs_database_id: String::new(),
            sessions_database_id: String::new(),
            projects_database_id: String::new(),
            sync_interval_minutes: 5,
        },
        privacy: PrivacyConfig {
            exclude_paths: vec![],
            exclude_titles: vec![],
            exclude_domains: vec![],
            redact_clipboard: true,
            clipboard_whitelist: vec![],
        },
    };

    let runtime = ActivityArchiveRuntime::new(config, &workspace_dir).unwrap();

    // Test shutdown
    let result = runtime.shutdown();

    assert!(result.is_ok());
}

#[test]
fn test_config_defaults() {
    let config = ActivityArchiveConfig::default();

    assert!(!config.enabled);
    assert!(config.database_path.is_none());
    assert!(config.collectors.window_focus);
    assert!(config.collectors.process_launch);
    assert!(config.collectors.browser_history);
    assert!(config.collectors.shell_activity);
    assert!(!config.collectors.file_activity);
    assert_eq!(config.collectors.poll_interval_seconds, 2);
    assert_eq!(config.sessionizer.idle_timeout_minutes, 30);
    assert_eq!(config.sessionizer.context_switch_threshold_minutes, 15);
    assert!(config.summarizer.enabled);
    assert!(config.summarizer.hourly_summary_enabled);
    assert!(config.summarizer.daily_log_enabled);
    assert!(config.summarizer.project_summary_enabled);
    assert!(!config.notion_sync.enabled);
    assert!(config.notion_sync.api_key.is_empty());
    assert!(config.notion_sync.daily_logs_database_id.is_empty());
    assert!(config.notion_sync.sessions_database_id.is_empty());
    assert!(config.notion_sync.projects_database_id.is_empty());
    assert_eq!(config.notion_sync.sync_interval_minutes, 5);
    assert!(config.privacy.exclude_paths.is_empty());
    assert!(config.privacy.exclude_titles.is_empty());
    assert!(config.privacy.exclude_domains.is_empty());
    assert!(config.privacy.redact_clipboard);
    assert!(config.privacy.clipboard_whitelist.is_empty());
}

#[test]
fn test_collector_config_defaults() {
    let config = CollectorConfig::default();

    assert!(config.window_focus);
    assert!(config.process_launch);
    assert!(config.browser_history);
    assert!(config.shell_activity);
    assert!(!config.file_activity);
    assert!(config.file_activity_folders.is_empty());
    assert_eq!(config.poll_interval_seconds, 2);
}

#[test]
fn test_sessionizer_config_defaults() {
    let config = SessionizerConfig::default();

    assert_eq!(config.idle_timeout_minutes, 30);
    assert_eq!(config.context_switch_threshold_minutes, 15);
}

#[test]
fn test_summarizer_config_defaults() {
    let config = SummarizerConfig::default();

    assert!(config.enabled);
    assert!(config.hourly_summary_enabled);
    assert!(config.daily_log_enabled);
    assert!(config.project_summary_enabled);
}

#[test]
fn test_notion_sync_config_defaults() {
    let config = NotionSyncConfig::default();

    assert!(!config.enabled);
    assert!(config.api_key.is_empty());
    assert!(config.daily_logs_database_id.is_empty());
    assert!(config.sessions_database_id.is_empty());
    assert!(config.projects_database_id.is_empty());
    assert_eq!(config.sync_interval_minutes, 5);
}

#[test]
fn test_privacy_config_defaults() {
    let config = PrivacyConfig::default();

    assert!(config.exclude_paths.is_empty());
    assert!(config.exclude_titles.is_empty());
    assert!(config.exclude_domains.is_empty());
    assert!(config.redact_clipboard);
    assert!(config.clipboard_whitelist.is_empty());
}

#[test]
fn test_database_path_resolution() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_dir = temp_dir.path().to_path_buf();

    let config = ActivityArchiveConfig {
        enabled: true,
        database_path: None,
        collectors: CollectorConfig::default(),
        sessionizer: SessionizerConfig::default(),
        summarizer: SummarizerConfig::default(),
        notion_sync: NotionSyncConfig::default(),
        privacy: PrivacyConfig::default(),
    };

    let runtime = ActivityArchiveRuntime::new(config, &workspace_dir).unwrap();

    // Database should be created in workspace directory
    let expected_db_path = workspace_dir.join("activity_archive.db");
    assert!(expected_db_path.exists());
}

#[test]
fn test_custom_database_path() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_dir = temp_dir.path().to_path_buf();
    let custom_db_path = temp_dir.path().join("custom.db").to_string();

    let config = ActivityArchiveConfig {
        enabled: true,
        database_path: Some(custom_db_path.clone()),
        collectors: CollectorConfig::default(),
        sessionizer: SessionizerConfig::default(),
        summarizer: SummarizerConfig::default(),
        notion_sync: NotionSyncConfig::default(),
        privacy: PrivacyConfig::default(),
    };

    let runtime = ActivityArchiveRuntime::new(config, &workspace_dir).unwrap();

    // Database should be created at custom path
    assert!(PathBuf::from(custom_db_path).exists());
}

#[test]
fn test_disabled_runtime() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_dir = temp_dir.path().to_path_buf();

    let config = ActivityArchiveConfig {
        enabled: false,
        database_path: None,
        collectors: CollectorConfig::default(),
        sessionizer: SessionizerConfig::default(),
        summarizer: SummarizerConfig::default(),
        notion_sync: NotionSyncConfig::default(),
        privacy: PrivacyConfig::default(),
    };

    let runtime = ActivityArchiveRuntime::new(config, &workspace_dir).unwrap();

    let status = runtime.status();

    assert!(!status.enabled);
}

#[test]
fn test_file_activity_folders() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_dir = temp_dir.path().to_path_buf();

    let test_folder = temp_dir.path().join("test_folder").to_path_buf();

    let config = ActivityArchiveConfig {
        enabled: true,
        database_path: None,
        collectors: CollectorConfig {
            window_focus: false,
            process_launch: false,
            browser_history: false,
            shell_activity: false,
            file_activity: true,
            file_activity_folders: vec![test_folder.to_string_lossy()],
            poll_interval_seconds: 2,
        },
        sessionizer: SessionizerConfig::default(),
        summarizer: SummarizerConfig::default(),
        notion_sync: NotionSyncConfig::default(),
        privacy: PrivacyConfig::default(),
    };

    let runtime = ActivityArchiveRuntime::new(config, &workspace_dir).unwrap();

    let status = runtime.status();

    // Should have file activity collector
    assert!(status.collectors_running >= 1);
}

#[test]
fn test_notion_sync_enabled() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_dir = temp_dir.path().to_path_buf();

    let config = ActivityArchiveConfig {
        enabled: true,
        database_path: None,
        collectors: CollectorConfig::default(),
        sessionizer: SessionizerConfig::default(),
        summarizer: SummarizerConfig::default(),
        notion_sync: NotionSyncConfig {
            enabled: true,
            api_key: "test_key".to_string(),
            daily_logs_database_id: "test_db".to_string(),
            sessions_database_id: "test_db".to_string(),
            projects_database_id: "test_db".to_string(),
            sync_interval_minutes: 10,
        },
        privacy: PrivacyConfig::default(),
    };

    let runtime = ActivityArchiveRuntime::new(config, &workspace_dir).unwrap();

    let status = runtime.status();

    assert!(status.enabled);
}

#[test]
fn test_privacy_exclusions() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_dir = temp_dir.path().to_path_buf();

    let config = ActivityArchiveConfig {
        enabled: true,
        database_path: None,
        collectors: CollectorConfig::default(),
        sessionizer: SessionizerConfig::default(),
        summarizer: SummarizerConfig::default(),
        notion_sync: NotionSyncConfig::default(),
        privacy: PrivacyConfig {
            exclude_paths: vec![
                "**/passwords/**".to_string(),
                "**/banking/**".to_string(),
            ],
            exclude_titles: vec![
                "*password*".to_string(),
                "*token*".to_string(),
            ],
            exclude_domains: vec![
                "*.bank.com".to_string(),
                "*.secure.com".to_string(),
            ],
            redact_clipboard: true,
            clipboard_whitelist: vec![],
        },
    };

    let runtime = ActivityArchiveRuntime::new(config, &workspace_dir).unwrap();

    let status = runtime.status();

    assert!(status.enabled);
}
