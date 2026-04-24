//! Integration point for the activity archive.
//!
//! Activity-archive is NOT a separate Windows service. It's a supervised
//! component of `ZeroClawDaemon`: when `config.activity_archive.enabled`,
//! [`start`] is spawned alongside the gateway, channels, cron, and heartbeat
//! supervisors. Its collectors and processing pipeline share the daemon's
//! tokio runtime and shut down when the daemon does.
//!
//! Windows-only for now because `zeroclaw-activity-archive`'s collectors
//! (`WindowFocusCollector` and friends) use Win32 APIs. If/when cross-platform
//! collectors land, this module gate can relax.

use anyhow::Result;
use zeroclaw_activity_archive::runtime::{
    ActivityArchiveConfig as RuntimeActivityArchiveConfig, ActivityArchiveRuntime,
    CollectorConfig as RuntimeCollectorConfig, NotionSyncConfig as RuntimeNotionSyncConfig,
    PrivacyConfig as RuntimePrivacyConfig, SessionizerConfig as RuntimeSessionizerConfig,
    SummarizerConfig as RuntimeSummarizerConfig,
};
use zeroclaw_config::schema::{
    ActivityArchiveConfig as SchemaActivityArchiveConfig, CollectorConfig as SchemaCollectorConfig,
    Config, NotionSyncConfig as SchemaNotionSyncConfig, PrivacyConfig as SchemaPrivacyConfig,
    SessionizerConfig as SchemaSessionizerConfig, SummarizerConfig as SchemaSummarizerConfig,
};

/// Build the archive runtime from the daemon config and run it until shutdown.
///
/// Invoked from a `spawn_component_supervisor` closure in `daemon::run`. The
/// supervisor handles restart-with-backoff if this returns an error.
pub async fn start(config: Config) -> Result<()> {
    let archive_config = to_runtime_config(config.activity_archive.clone());
    let runtime = ActivityArchiveRuntime::new(archive_config, &config.workspace_dir)?;
    runtime.run().await
}

fn to_runtime_config(c: SchemaActivityArchiveConfig) -> RuntimeActivityArchiveConfig {
    RuntimeActivityArchiveConfig {
        enabled: c.enabled,
        database_path: c.database_path,
        collectors: to_runtime_collectors(c.collectors),
        sessionizer: to_runtime_sessionizer(c.sessionizer),
        summarizer: to_runtime_summarizer(c.summarizer),
        notion_sync: to_runtime_notion_sync(c.notion_sync),
        privacy: to_runtime_privacy(c.privacy),
    }
}

fn to_runtime_collectors(c: SchemaCollectorConfig) -> RuntimeCollectorConfig {
    RuntimeCollectorConfig {
        window_focus: c.window_focus,
        process_launch: c.process_launch,
        browser_history: c.browser_history,
        shell_activity: c.shell_activity,
        file_activity: c.file_activity,
        file_activity_folders: c.file_activity_folders,
        poll_interval_seconds: c.poll_interval_seconds,
        idle_threshold_seconds: c.idle_threshold_seconds,
    }
}

fn to_runtime_sessionizer(c: SchemaSessionizerConfig) -> RuntimeSessionizerConfig {
    RuntimeSessionizerConfig {
        idle_timeout_minutes: c.idle_timeout_minutes,
        context_switch_threshold_minutes: c.context_switch_threshold_minutes,
    }
}

fn to_runtime_summarizer(c: SchemaSummarizerConfig) -> RuntimeSummarizerConfig {
    RuntimeSummarizerConfig {
        enabled: c.enabled,
        hourly_summary_enabled: c.hourly_summary_enabled,
        daily_log_enabled: c.daily_log_enabled,
        project_summary_enabled: c.project_summary_enabled,
    }
}

fn to_runtime_notion_sync(c: SchemaNotionSyncConfig) -> RuntimeNotionSyncConfig {
    RuntimeNotionSyncConfig {
        enabled: c.enabled,
        api_key: c.api_key,
        daily_logs_database_id: c.daily_logs_database_id,
        sessions_database_id: c.sessions_database_id,
        projects_database_id: c.projects_database_id,
        sync_interval_minutes: c.sync_interval_minutes,
    }
}

fn to_runtime_privacy(c: SchemaPrivacyConfig) -> RuntimePrivacyConfig {
    RuntimePrivacyConfig {
        exclude_paths: c.exclude_paths,
        exclude_titles: c.exclude_titles,
        exclude_domains: c.exclude_domains,
        redact_clipboard: c.redact_clipboard,
        clipboard_whitelist: c.clipboard_whitelist,
    }
}
