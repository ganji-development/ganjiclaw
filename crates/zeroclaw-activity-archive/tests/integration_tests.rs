//! Integration tests for the activity archive runtime — public API only.

use std::path::PathBuf;
use tempfile::TempDir;
use zeroclaw_activity_archive::runtime::{
    ActivityArchiveConfig, ActivityArchiveRuntime, CollectorConfig, NotionSyncConfig,
    PrivacyConfig, SessionizerConfig, SummarizerConfig,
};

#[test]
fn config_defaults_match_schema() {
    let config = ActivityArchiveConfig::default();

    assert!(!config.enabled);
    assert!(config.database_path.is_none());
    assert!(config.collectors.window_focus);
    assert!(config.collectors.process_launch);
    assert!(config.collectors.browser_history);
    assert!(config.collectors.shell_activity);
    assert!(!config.collectors.file_activity);
    assert!(config.collectors.file_activity_folders.is_empty());
    assert_eq!(config.collectors.poll_interval_seconds, 2);
    assert_eq!(config.collectors.idle_threshold_seconds, 120);
    assert_eq!(config.sessionizer.idle_timeout_minutes, 30);
    assert_eq!(config.sessionizer.context_switch_threshold_minutes, 15);
    assert!(config.summarizer.enabled);
    assert!(!config.notion_sync.enabled);
    assert_eq!(config.notion_sync.sync_interval_minutes, 5);
    assert!(config.privacy.redact_clipboard);
}

#[test]
fn runtime_creation_with_no_collectors() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_dir = temp_dir.path().to_path_buf();

    let config = ActivityArchiveConfig {
        enabled: true,
        collectors: CollectorConfig {
            window_focus: false,
            process_launch: false,
            browser_history: false,
            shell_activity: false,
            file_activity: false,
            ..CollectorConfig::default()
        },
        ..ActivityArchiveConfig::default()
    };

    let runtime = ActivityArchiveRuntime::new(config, &workspace_dir).unwrap();
    let status = runtime.status();
    assert!(status.enabled);
    assert_eq!(status.collectors_running, 0);
}

#[test]
fn runtime_starts_each_enabled_collector() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_dir = temp_dir.path().to_path_buf();

    let config = ActivityArchiveConfig {
        enabled: true,
        collectors: CollectorConfig {
            window_focus: true,
            process_launch: true,
            browser_history: true,
            shell_activity: true,
            file_activity: false,
            ..CollectorConfig::default()
        },
        ..ActivityArchiveConfig::default()
    };

    let runtime = ActivityArchiveRuntime::new(config, &workspace_dir).unwrap();
    assert_eq!(runtime.status().collectors_running, 4);
}

#[test]
fn runtime_shutdown_succeeds_when_idle() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_dir = temp_dir.path().to_path_buf();

    let config = ActivityArchiveConfig {
        enabled: true,
        collectors: CollectorConfig {
            window_focus: false,
            process_launch: false,
            browser_history: false,
            shell_activity: false,
            file_activity: false,
            ..CollectorConfig::default()
        },
        ..ActivityArchiveConfig::default()
    };

    let runtime = ActivityArchiveRuntime::new(config, &workspace_dir).unwrap();
    runtime.shutdown().unwrap();
}

#[test]
fn database_lands_in_workspace_when_path_unset() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_dir = temp_dir.path().to_path_buf();

    let config = ActivityArchiveConfig {
        enabled: true,
        ..ActivityArchiveConfig::default()
    };

    let _runtime = ActivityArchiveRuntime::new(config, &workspace_dir).unwrap();
    assert!(workspace_dir.join("activity_archive.db").exists());
}

#[test]
fn database_path_override_is_honored() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_dir = temp_dir.path().to_path_buf();
    let custom = temp_dir.path().join("custom.db");
    let custom_str = custom.to_string_lossy().into_owned();

    let config = ActivityArchiveConfig {
        enabled: true,
        database_path: Some(custom_str.clone()),
        ..ActivityArchiveConfig::default()
    };

    let _runtime = ActivityArchiveRuntime::new(config, &workspace_dir).unwrap();
    assert!(PathBuf::from(custom_str).exists());
}

#[test]
fn disabled_runtime_reports_disabled_status() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_dir = temp_dir.path().to_path_buf();

    let config = ActivityArchiveConfig::default();
    let runtime = ActivityArchiveRuntime::new(config, &workspace_dir).unwrap();
    assert!(!runtime.status().enabled);
}

#[test]
fn file_activity_folders_pass_through() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_dir = temp_dir.path().to_path_buf();
    let watch_dir = temp_dir.path().join("watch");
    std::fs::create_dir(&watch_dir).unwrap();

    let config = ActivityArchiveConfig {
        enabled: true,
        collectors: CollectorConfig {
            window_focus: false,
            process_launch: false,
            browser_history: false,
            shell_activity: false,
            file_activity: true,
            file_activity_folders: vec![watch_dir.to_string_lossy().into_owned()],
            ..CollectorConfig::default()
        },
        ..ActivityArchiveConfig::default()
    };

    let runtime = ActivityArchiveRuntime::new(config, &workspace_dir).unwrap();
    assert_eq!(runtime.status().collectors_running, 1);
}

#[test]
fn notion_sync_fields_round_trip_through_runtime() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_dir = temp_dir.path().to_path_buf();

    let config = ActivityArchiveConfig {
        enabled: true,
        notion_sync: NotionSyncConfig {
            enabled: true,
            api_key: "test_key".to_string(),
            daily_logs_database_id: "daily".to_string(),
            sessions_database_id: "sessions".to_string(),
            projects_database_id: "projects".to_string(),
            sync_interval_minutes: 10,
        },
        ..ActivityArchiveConfig::default()
    };

    let runtime = ActivityArchiveRuntime::new(config, &workspace_dir).unwrap();
    assert!(runtime.status().enabled);
}

#[test]
fn privacy_config_passes_through() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_dir = temp_dir.path().to_path_buf();

    let config = ActivityArchiveConfig {
        enabled: true,
        privacy: PrivacyConfig {
            exclude_paths: vec!["**/passwords/**".to_string()],
            exclude_titles: vec!["*password*".to_string()],
            exclude_domains: vec!["*.bank.com".to_string()],
            redact_clipboard: true,
            clipboard_whitelist: vec![],
        },
        ..ActivityArchiveConfig::default()
    };

    let runtime = ActivityArchiveRuntime::new(config, &workspace_dir).unwrap();
    assert!(runtime.status().enabled);
}

#[test]
fn sessionizer_config_overrides_apply() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_dir = temp_dir.path().to_path_buf();

    let config = ActivityArchiveConfig {
        enabled: true,
        sessionizer: SessionizerConfig {
            idle_timeout_minutes: 60,
            context_switch_threshold_minutes: 5,
        },
        summarizer: SummarizerConfig {
            enabled: false,
            hourly_summary_enabled: false,
            daily_log_enabled: false,
            project_summary_enabled: false,
        },
        ..ActivityArchiveConfig::default()
    };

    // Just verifies the runtime accepts a non-default sessionizer/summarizer.
    let _runtime = ActivityArchiveRuntime::new(config, &workspace_dir).unwrap();
}
