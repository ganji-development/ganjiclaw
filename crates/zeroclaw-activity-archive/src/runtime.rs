//! Main runtime orchestration for the activity archive.
//!
//! Coordinates collectors, normalizer, sessionizer, summarizer, and Notion sync.

use crate::collectors::{
    WindowFocusCollector, ProcessLaunchCollector, BrowserHistoryCollector,
    ShellActivityCollector, FileActivityCollector,
};
use crate::collector::Collector;
use crate::normalizer::Normalizer;
use crate::sessionizer::Sessionizer;
use crate::summarizer::Summarizer;
use crate::notion_sync::NotionSync;
use crate::privacy::PrivacyManager;
use crate::schema::{open_connection, init_schema};
use parking_lot::Mutex;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use anyhow::Result;
use tokio::sync::broadcast;
use futures::StreamExt;

/// Activity archive runtime.
///
/// Orchestrates all components of the activity archive system.
#[allow(dead_code)]
pub struct ActivityArchiveRuntime {
    config: ActivityArchiveConfig,
    db: Arc<Mutex<Connection>>,
    collectors: Vec<Box<dyn Collector>>,
    normalizer: Arc<Normalizer>,
    sessionizer: Arc<Sessionizer>,
    summarizer: Arc<Summarizer>,
    notion_sync: Arc<NotionSync>,
    privacy_manager: Arc<PrivacyManager>,
    shutdown_tx: broadcast::Sender<()>,
}

/// Activity archive configuration.
#[derive(Debug, Clone)]
pub struct ActivityArchiveConfig {
    pub enabled: bool,
    pub database_path: Option<String>,
    pub collectors: CollectorConfig,
    pub sessionizer: SessionizerConfig,
    pub summarizer: SummarizerConfig,
    pub notion_sync: NotionSyncConfig,
    pub privacy: PrivacyConfig,
}

/// Collector configuration.
#[derive(Debug, Clone)]
pub struct CollectorConfig {
    pub window_focus: bool,
    pub process_launch: bool,
    pub browser_history: bool,
    pub shell_activity: bool,
    pub file_activity: bool,
    pub file_activity_folders: Vec<String>,
    pub poll_interval_seconds: u64,
    pub idle_threshold_seconds: u64,
}

/// Sessionizer configuration.
#[derive(Debug, Clone)]
pub struct SessionizerConfig {
    pub idle_timeout_minutes: u64,
    pub context_switch_threshold_minutes: u64,
}

/// Summarizer configuration.
#[derive(Debug, Clone)]
pub struct SummarizerConfig {
    pub enabled: bool,
    pub hourly_summary_enabled: bool,
    pub daily_log_enabled: bool,
    pub project_summary_enabled: bool,
}

/// Notion sync configuration.
#[derive(Debug, Clone)]
pub struct NotionSyncConfig {
    pub enabled: bool,
    pub api_key: String,
    pub daily_logs_database_id: String,
    pub sessions_database_id: String,
    pub projects_database_id: String,
    pub sync_interval_minutes: u64,
}

/// Privacy configuration.
#[derive(Debug, Clone)]
pub struct PrivacyConfig {
    pub exclude_paths: Vec<String>,
    pub exclude_titles: Vec<String>,
    pub exclude_domains: Vec<String>,
    pub redact_clipboard: bool,
    pub clipboard_whitelist: Vec<String>,
}

impl ActivityArchiveRuntime {
    /// Create a new activity archive runtime.
    ///
    /// # Arguments
    ///
    /// * `config` - Activity archive configuration
    /// * `workspace_dir` - Workspace directory for database storage
    pub fn new(config: ActivityArchiveConfig, workspace_dir: &PathBuf) -> Result<Self> {
        // Determine database path
        let db_path = if let Some(path) = &config.database_path {
            PathBuf::from(path)
        } else {
            workspace_dir.join("activity_archive.db")
        };

        // Initialize database
        let conn = open_connection(&db_path, None)?;
        init_schema(&conn)?;

        let db = Arc::new(Mutex::new(conn));

        // Create collectors
        let mut collectors: Vec<Box<dyn Collector>> = Vec::new();

        if config.collectors.window_focus {
            collectors.push(Box::new(WindowFocusCollector::new(
                db_path.clone(),
                config.collectors.poll_interval_seconds,
                config.collectors.idle_threshold_seconds,
            )));
        }

        if config.collectors.process_launch {
            collectors.push(Box::new(ProcessLaunchCollector::new(db_path.clone())));
        }

        if config.collectors.browser_history {
            collectors.push(Box::new(BrowserHistoryCollector::new(db_path.clone())));
        }

        if config.collectors.shell_activity {
            collectors.push(Box::new(ShellActivityCollector::new(db_path.clone())));
        }

        if config.collectors.file_activity {
            let folders: Vec<PathBuf> = config.collectors.file_activity_folders
                .iter()
                .map(|s| PathBuf::from(s))
                .collect();
            collectors.push(Box::new(FileActivityCollector::new(db_path.clone(), folders)));
        }

        // Create normalizer
        let normalizer = Arc::new(Normalizer::new(db.clone()));
        normalizer.load_privacy_rules()?;

        // Create sessionizer
        let sessionizer = Arc::new(Sessionizer::new(
            db.clone(),
            config.sessionizer.idle_timeout_minutes,
            config.sessionizer.context_switch_threshold_minutes,
        ));

        // Create summarizer
        let summarizer = Arc::new(Summarizer::new(db.clone()));

        // Create Notion sync
        let notion_sync = Arc::new(NotionSync::new(
            db.clone(),
            config.notion_sync.api_key.clone(),
            config.notion_sync.daily_logs_database_id.clone(),
            config.notion_sync.sessions_database_id.clone(),
            config.notion_sync.projects_database_id.clone(),
            Duration::from_secs(config.notion_sync.sync_interval_minutes * 60),
        ));

        // Create privacy manager
        let privacy_manager = Arc::new(PrivacyManager::new(db.clone()));
        privacy_manager.initialize_default_rules()?;

        // Create shutdown channel
        let (shutdown_tx, _) = broadcast::channel(1);

        Ok(Self {
            config,
            db,
            collectors,
            normalizer,
            sessionizer,
            summarizer,
            notion_sync,
            privacy_manager,
            shutdown_tx,
        })
    }

    /// Run the activity archive runtime.
    ///
    /// This method starts all components and runs until shutdown is requested.
    pub async fn run(&self) -> Result<()> {
        tracing::info!("Starting activity archive runtime");

        // Start collectors
        let mut collector_tasks = Vec::new();
        for collector in &self.collectors {
            let name = collector.name().to_string();
            let stream = collector.start().await?;

            let normalizer = self.normalizer.clone();
            let mut shutdown_rx = self.shutdown_tx.subscribe();

            let task = tokio::spawn(async move {
                tracing::info!("Starting collector: {}", name);
                let mut stream = stream;

                loop {
                    tokio::select! {
                        result = stream.next() => {
                            match result {
                                Some(raw_event) => {
                                    if let Err(e) = normalizer.process_raw_event(&raw_event) {
                                        tracing::error!("Failed to process raw event: {}", e);
                                    }
                                }
                                None => {
                                    tracing::info!("Collector {} stream ended", name);
                                    break;
                                }
                            }
                        }
                        _ = shutdown_rx.recv() => {
                            tracing::info!("Collector {} shutting down", name);
                            break;
                        }
                    }
                }
            });

            collector_tasks.push(task);
        }

        // Start sessionizer (periodic)
        let sessionizer = self.sessionizer.clone();
        let mut sessionizer_shutdown_rx = self.shutdown_tx.subscribe();
        let sessionizer_task = tokio::spawn(async move {
            tracing::info!("Starting sessionizer");
            let mut interval = tokio::time::interval(Duration::from_secs(60));

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if let Err(e) = sessionizer.update_sessions() {
                            tracing::error!("Failed to update sessions: {}", e);
                        }
                    }
                    _ = sessionizer_shutdown_rx.recv() => {
                        tracing::info!("Sessionizer shutting down");
                        break;
                    }
                }
            }
        });

        // Start summarizer (periodic)
        let summarizer = self.summarizer.clone();
        let summarizer_config = self.config.summarizer.clone();
        let mut summarizer_shutdown_rx = self.shutdown_tx.subscribe();
        let summarizer_task = tokio::spawn(async move {
            tracing::info!("Starting summarizer");
            let mut interval = tokio::time::interval(Duration::from_secs(300)); // 5 minutes

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if summarizer_config.enabled {
                            // Generate hourly summaries
                            if summarizer_config.hourly_summary_enabled {
                                let now = chrono::Utc::now();
                                let hour = now - chrono::Duration::hours(1);
                                if let Err(e) = summarizer.generate_hourly_summary(hour) {
                                    tracing::error!("Failed to generate hourly summary: {}", e);
                                }
                            }

                            // Generate daily logs
                            if summarizer_config.daily_log_enabled {
                                let today = chrono::Utc::now().date_naive();
                                if let Err(e) = summarizer.generate_daily_log(today) {
                                    tracing::error!("Failed to generate daily log: {}", e);
                                }
                            }
                        }
                    }
                    _ = summarizer_shutdown_rx.recv() => {
                        tracing::info!("Summarizer shutting down");
                        break;
                    }
                }
            }
        });

        // Start Notion sync (if enabled)
        let notion_sync_task = if self.config.notion_sync.enabled {
            let notion_sync = self.notion_sync.clone();
            let notion_sync_shutdown_rx = self.shutdown_tx.subscribe();
            Some(tokio::spawn(async move {
                tracing::info!("Starting Notion sync");
                if let Err(e) = notion_sync.process_queue().await {
                    tracing::error!("Notion sync failed: {}", e);
                }
                drop(notion_sync_shutdown_rx);
            }))
        } else {
            None
        };

        // Wait for shutdown signal
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        shutdown_rx.recv().await?;

        tracing::info!("Shutting down activity archive runtime");

        // Cancel all tasks
        for task in collector_tasks {
            task.abort();
        }
        sessionizer_task.abort();
        summarizer_task.abort();
        if let Some(task) = notion_sync_task {
            task.abort();
        }

        tracing::info!("Activity archive runtime stopped");
        Ok(())
    }

    /// Shutdown the runtime.
    pub fn shutdown(&self) -> Result<()> {
        tracing::info!("Requesting shutdown");
        let _ = self.shutdown_tx.send(());
        Ok(())
    }

    /// Get runtime status.
    pub fn status(&self) -> RuntimeStatus {
        RuntimeStatus {
            enabled: self.config.enabled,
            collectors_running: self.collectors.len(),
            database_path: self.config.database_path.clone(),
        }
    }
}

/// Runtime status.
#[derive(Debug, Clone)]
pub struct RuntimeStatus {
    pub enabled: bool,
    pub collectors_running: usize,
    pub database_path: Option<String>,
}
