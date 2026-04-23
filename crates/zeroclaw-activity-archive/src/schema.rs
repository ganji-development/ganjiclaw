//! Database schema and data structures for the activity archive.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::Path;
use uuid::Uuid;

/// Maximum allowed open timeout (seconds) to avoid unreasonable waits.
const SQLITE_OPEN_TIMEOUT_CAP_SECS: u64 = 300;

/// Initialize the database schema.
pub fn init_schema(conn: &Connection) -> Result<()> {
    // Raw events table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS raw_events (
            id TEXT PRIMARY KEY,
            source TEXT NOT NULL,
            raw_payload TEXT NOT NULL,
            ingested_at TEXT NOT NULL,
            processed_at TEXT
        )",
        [],
    )?;

    // Events table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS events (
            id TEXT PRIMARY KEY,
            ts_utc TEXT NOT NULL,
            ts_local TEXT NOT NULL,
            source TEXT NOT NULL,
            event_type TEXT NOT NULL,
            actor TEXT,
            host TEXT,
            app TEXT,
            title TEXT,
            path TEXT,
            details_json TEXT,
            sensitivity INTEGER DEFAULT 0,
            project_key TEXT,
            session_id TEXT,
            hash TEXT,
            raw_ref TEXT,
            created_at TEXT NOT NULL
        )",
        [],
    )?;

    // Sessions table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS sessions (
            id TEXT PRIMARY KEY,
            start_ts_utc TEXT NOT NULL,
            end_ts_utc TEXT,
            label TEXT,
            project_key TEXT,
            tags TEXT,
            summary TEXT,
            event_count INTEGER DEFAULT 0,
            created_at TEXT NOT NULL
        )",
        [],
    )?;

    // Entities table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS entities (
            id TEXT PRIMARY KEY,
            entity_type TEXT NOT NULL,
            name TEXT NOT NULL,
            metadata_json TEXT,
            first_seen TEXT NOT NULL,
            last_seen TEXT NOT NULL,
            occurrence_count INTEGER DEFAULT 1
        )",
        [],
    )?;

    // Event-entity map table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS event_entity_map (
            event_id TEXT NOT NULL,
            entity_id TEXT NOT NULL,
            relationship_type TEXT,
            PRIMARY KEY (event_id, entity_id)
        )",
        [],
    )?;

    // Summaries table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS summaries (
            id TEXT PRIMARY KEY,
            summary_type TEXT NOT NULL,
            period_start TEXT NOT NULL,
            period_end TEXT NOT NULL,
            project_key TEXT,
            topic TEXT,
            content TEXT NOT NULL,
            metrics_json TEXT,
            created_at TEXT NOT NULL
        )",
        [],
    )?;

    // Artifacts table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS artifacts (
            id TEXT PRIMARY KEY,
            artifact_type TEXT NOT NULL,
            path TEXT NOT NULL,
            size_bytes INTEGER,
            created_at TEXT NOT NULL,
            metadata_json TEXT
        )",
        [],
    )?;

    // Notion sync queue table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS notion_sync_queue (
            id TEXT PRIMARY KEY,
            sync_type TEXT NOT NULL,
            target_id TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            status TEXT DEFAULT 'pending',
            notion_page_id TEXT,
            error_message TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            retry_count INTEGER DEFAULT 0
        )",
        [],
    )?;

    // Ingestion offsets table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS ingestion_offsets (
            collector_name TEXT PRIMARY KEY,
            offset_value TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
        [],
    )?;

    // Privacy rules table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS privacy_rules (
            id TEXT PRIMARY KEY,
            rule_type TEXT NOT NULL,
            pattern TEXT NOT NULL,
            action TEXT NOT NULL,
            created_at TEXT NOT NULL
        )",
        [],
    )?;

    // Create indexes
    create_indexes(conn)?;

    Ok(())
}

/// Create database indexes for performance.
fn create_indexes(conn: &Connection) -> Result<()> {
    let indexes = vec![
        ("idx_events_ts_utc", "events", "ts_utc"),
        ("idx_events_event_type", "events", "event_type"),
        ("idx_events_session_id", "events", "session_id"),
        ("idx_events_project_key", "events", "project_key"),
        ("idx_sessions_start_ts_utc", "sessions", "start_ts_utc"),
        ("idx_summaries_period", "summaries", "summary_type, period_start, period_end"),
        ("idx_notion_sync_queue_status", "notion_sync_queue", "status, created_at"),
        ("idx_entities_type", "entities", "entity_type"),
        ("idx_entities_name", "entities", "name"),
    ];

    for (name, table, columns) in indexes {
        conn.execute(
            &format!("CREATE INDEX IF NOT EXISTS {name} ON {table} ({columns})"),
            [],
        )?;
    }

    Ok(())
}

/// Open a database connection with optional timeout.
pub fn open_connection(db_path: &Path, open_timeout_secs: Option<u64>) -> Result<Connection> {
    let timeout_secs = open_timeout_secs.unwrap_or(SQLITE_OPEN_TIMEOUT_CAP_SECS).min(SQLITE_OPEN_TIMEOUT_CAP_SECS);

    let db_path_str = db_path.to_str().context("Invalid database path")?;

    let conn = Connection::open(db_path_str)?;

    // Configure SQLite for performance
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous  = NORMAL;
         PRAGMA mmap_size    = 8388608;
         PRAGMA cache_size   = -2000;
         PRAGMA temp_store   = MEMORY;",
    )?;

    // Set busy timeout
    conn.busy_timeout(std::time::Duration::from_secs(timeout_secs))?;

    Ok(conn)
}

/// Raw event from a collector.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawEvent {
    pub id: String,
    pub source: String,
    pub payload: serde_json::Value,
    pub timestamp: DateTime<Utc>,
}

impl RawEvent {
    pub fn new(source: String, payload: serde_json::Value) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            source,
            payload,
            timestamp: Utc::now(),
        }
    }
}

/// Canonical normalized event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub ts_utc: DateTime<Utc>,
    pub ts_local: DateTime<chrono::Local>,
    pub source: String,
    pub event_type: EventType,
    pub actor: Option<String>,
    pub host: Option<String>,
    pub app: Option<String>,
    pub title: Option<String>,
    pub path: Option<String>,
    pub details: serde_json::Value,
    pub sensitivity: u8,
    pub project_key: Option<String>,
    pub session_id: Option<String>,
    pub hash: Option<String>,
    pub raw_ref: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl Event {
    pub fn new(source: String, event_type: EventType) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            ts_utc: now,
            ts_local: chrono::Local::now(),
            source,
            event_type,
            actor: None,
            host: None,
            app: None,
            title: None,
            path: None,
            details: serde_json::Value::Object(serde_json::Map::new()),
            sensitivity: 0,
            project_key: None,
            session_id: None,
            hash: None,
            raw_ref: None,
            created_at: now,
        }
    }

    /// Generate a hash for deduplication.
    pub fn generate_hash(&self) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        self.event_type.hash(&mut hasher);
        self.app.hash(&mut hasher);
        self.title.hash(&mut hasher);
        self.path.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }
}

/// Event types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    WindowFocus,
    ProcessStart,
    ProcessExit,
    BrowserVisit,
    ShellCommand,
    FileCreate,
    FileModify,
    FileDelete,
    FileRename,
    SystemEvent,
}

impl EventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            EventType::WindowFocus => "window_focus",
            EventType::ProcessStart => "process_start",
            EventType::ProcessExit => "process_exit",
            EventType::BrowserVisit => "browser_visit",
            EventType::ShellCommand => "shell_command",
            EventType::FileCreate => "file_create",
            EventType::FileModify => "file_modify",
            EventType::FileDelete => "file_delete",
            EventType::FileRename => "file_rename",
            EventType::SystemEvent => "system_event",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "window_focus" => Some(EventType::WindowFocus),
            "process_start" => Some(EventType::ProcessStart),
            "process_exit" => Some(EventType::ProcessExit),
            "browser_visit" => Some(EventType::BrowserVisit),
            "shell_command" => Some(EventType::ShellCommand),
            "file_create" => Some(EventType::FileCreate),
            "file_modify" => Some(EventType::FileModify),
            "file_delete" => Some(EventType::FileDelete),
            "file_rename" => Some(EventType::FileRename),
            "system_event" => Some(EventType::SystemEvent),
            _ => None,
        }
    }
}

/// Session (grouped activity block).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub start_ts_utc: DateTime<Utc>,
    pub end_ts_utc: Option<DateTime<Utc>>,
    pub label: SessionLabel,
    pub project_key: Option<String>,
    pub tags: Vec<String>,
    pub summary: Option<String>,
    pub event_count: u32,
    pub created_at: DateTime<Utc>,
}

impl Session {
    pub fn new(start_ts_utc: DateTime<Utc>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            start_ts_utc,
            end_ts_utc: None,
            label: SessionLabel::Unknown,
            project_key: None,
            tags: Vec::new(),
            summary: None,
            event_count: 0,
            created_at: Utc::now(),
        }
    }
}

/// Session labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionLabel {
    Coding,
    Research,
    Admin,
    Messaging,
    Music,
    Writing,
    Design,
    Unknown,
}

impl SessionLabel {
    pub fn as_str(&self) -> &'static str {
        match self {
            SessionLabel::Coding => "coding",
            SessionLabel::Research => "research",
            SessionLabel::Admin => "admin",
            SessionLabel::Messaging => "messaging",
            SessionLabel::Music => "music",
            SessionLabel::Writing => "writing",
            SessionLabel::Design => "design",
            SessionLabel::Unknown => "unknown",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "coding" => Some(SessionLabel::Coding),
            "research" => Some(SessionLabel::Research),
            "admin" => Some(SessionLabel::Admin),
            "messaging" => Some(SessionLabel::Messaging),
            "music" => Some(SessionLabel::Music),
            "writing" => Some(SessionLabel::Writing),
            "design" => Some(SessionLabel::Design),
            "unknown" => Some(SessionLabel::Unknown),
            _ => None,
        }
    }
}

/// Entity (app, project, folder, domain, repo, person, document).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub id: String,
    pub entity_type: EntityType,
    pub name: String,
    pub metadata: serde_json::Value,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub occurrence_count: u32,
}

impl Entity {
    pub fn new(entity_type: EntityType, name: String) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            entity_type,
            name,
            metadata: serde_json::Value::Object(serde_json::Map::new()),
            first_seen: now,
            last_seen: now,
            occurrence_count: 1,
        }
    }
}

/// Entity types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityType {
    App,
    Project,
    Folder,
    Domain,
    Repo,
    Person,
    Document,
}

impl EntityType {
    pub fn as_str(&self) -> &'static str {
        match self {
            EntityType::App => "app",
            EntityType::Project => "project",
            EntityType::Folder => "folder",
            EntityType::Domain => "domain",
            EntityType::Repo => "repo",
            EntityType::Person => "person",
            EntityType::Document => "document",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "app" => Some(EntityType::App),
            "project" => Some(EntityType::Project),
            "folder" => Some(EntityType::Folder),
            "domain" => Some(EntityType::Domain),
            "repo" => Some(EntityType::Repo),
            "person" => Some(EntityType::Person),
            "document" => Some(EntityType::Document),
            _ => None,
        }
    }
}

/// Summary (hourly, daily, weekly, project, topic).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    pub id: String,
    pub summary_type: SummaryType,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub project_key: Option<String>,
    pub topic: Option<String>,
    pub content: String,
    pub metrics: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

impl Summary {
    pub fn new(summary_type: SummaryType, period_start: DateTime<Utc>, period_end: DateTime<Utc>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            summary_type,
            period_start,
            period_end,
            project_key: None,
            topic: None,
            content: String::new(),
            metrics: serde_json::Value::Object(serde_json::Map::new()),
            created_at: Utc::now(),
        }
    }
}

/// Summary types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SummaryType {
    Hourly,
    Daily,
    Weekly,
    Project,
    Topic,
}

impl SummaryType {
    pub fn as_str(&self) -> &'static str {
        match self {
            SummaryType::Hourly => "hourly",
            SummaryType::Daily => "daily",
            SummaryType::Weekly => "weekly",
            SummaryType::Project => "project",
            SummaryType::Topic => "topic",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "hourly" => Some(SummaryType::Hourly),
            "daily" => Some(SummaryType::Daily),
            "weekly" => Some(SummaryType::Weekly),
            "project" => Some(SummaryType::Project),
            "topic" => Some(SummaryType::Topic),
            _ => None,
        }
    }
}

/// Artifact (linked file, snapshot, export).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub id: String,
    pub artifact_type: ArtifactType,
    pub path: String,
    pub size_bytes: Option<u64>,
    pub created_at: DateTime<Utc>,
    pub metadata: serde_json::Value,
}

impl Artifact {
    pub fn new(artifact_type: ArtifactType, path: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            artifact_type,
            path,
            size_bytes: None,
            created_at: Utc::now(),
            metadata: serde_json::Value::Object(serde_json::Map::new()),
        }
    }
}

/// Artifact types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactType {
    Screenshot,
    FileExport,
    LogExport,
}

impl ArtifactType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ArtifactType::Screenshot => "screenshot",
            ArtifactType::FileExport => "file_export",
            ArtifactType::LogExport => "log_export",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "screenshot" => Some(ArtifactType::Screenshot),
            "file_export" => Some(ArtifactType::FileExport),
            "log_export" => Some(ArtifactType::LogExport),
            _ => None,
        }
    }
}

/// Notion sync queue item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotionSyncItem {
    pub id: String,
    pub sync_type: NotionSyncType,
    pub target_id: String,
    pub payload: serde_json::Value,
    pub status: SyncStatus,
    pub notion_page_id: Option<String>,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub retry_count: u32,
}

impl NotionSyncItem {
    pub fn new(sync_type: NotionSyncType, target_id: String, payload: serde_json::Value) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            sync_type,
            target_id,
            payload,
            status: SyncStatus::Pending,
            notion_page_id: None,
            error_message: None,
            created_at: now,
            updated_at: now,
            retry_count: 0,
        }
    }
}

/// Notion sync types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotionSyncType {
    DailyLog,
    Session,
    Project,
    Pattern,
    Decision,
}

impl NotionSyncType {
    pub fn as_str(&self) -> &'static str {
        match self {
            NotionSyncType::DailyLog => "daily_log",
            NotionSyncType::Session => "session",
            NotionSyncType::Project => "project",
            NotionSyncType::Pattern => "pattern",
            NotionSyncType::Decision => "decision",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "daily_log" => Some(NotionSyncType::DailyLog),
            "session" => Some(NotionSyncType::Session),
            "project" => Some(NotionSyncType::Project),
            "pattern" => Some(NotionSyncType::Pattern),
            "decision" => Some(NotionSyncType::Decision),
            _ => None,
        }
    }
}

/// Sync status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncStatus {
    Pending,
    Syncing,
    Synced,
    Failed,
}

impl SyncStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            SyncStatus::Pending => "pending",
            SyncStatus::Syncing => "syncing",
            SyncStatus::Synced => "synced",
            SyncStatus::Failed => "failed",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(SyncStatus::Pending),
            "syncing" => Some(SyncStatus::Syncing),
            "synced" => Some(SyncStatus::Synced),
            "failed" => Some(SyncStatus::Failed),
            _ => None,
        }
    }
}

/// Privacy rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyRule {
    pub id: String,
    pub rule_type: PrivacyRuleType,
    pub pattern: String,
    pub action: PrivacyAction,
    pub created_at: DateTime<Utc>,
}

impl PrivacyRule {
    pub fn new(rule_type: PrivacyRuleType, pattern: String, action: PrivacyAction) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            rule_type,
            pattern,
            action,
            created_at: Utc::now(),
        }
    }
}

/// Privacy rule types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyRuleType {
    ExcludePath,
    ExcludeTitle,
    ExcludeDomain,
    Redaction,
}

impl PrivacyRuleType {
    pub fn as_str(&self) -> &'static str {
        match self {
            PrivacyRuleType::ExcludePath => "exclude_path",
            PrivacyRuleType::ExcludeTitle => "exclude_title",
            PrivacyRuleType::ExcludeDomain => "exclude_domain",
            PrivacyRuleType::Redaction => "redaction",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "exclude_path" => Some(PrivacyRuleType::ExcludePath),
            "exclude_title" => Some(PrivacyRuleType::ExcludeTitle),
            "exclude_domain" => Some(PrivacyRuleType::ExcludeDomain),
            "redaction" => Some(PrivacyRuleType::Redaction),
            _ => None,
        }
    }
}

/// Privacy actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyAction {
    Exclude,
    Redact,
    Hash,
}

impl PrivacyAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            PrivacyAction::Exclude => "exclude",
            PrivacyAction::Redact => "redact",
            PrivacyAction::Hash => "hash",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "exclude" => Some(PrivacyAction::Exclude),
            "redact" => Some(PrivacyAction::Redact),
            "hash" => Some(PrivacyAction::Hash),
            _ => None,
        }
    }
}

/// Ingestion offset for a collector.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestionOffset {
    pub collector_name: String,
    pub offset_value: String,
    pub updated_at: DateTime<Utc>,
}

impl IngestionOffset {
    pub fn new(collector_name: String, offset_value: String) -> Self {
        Self {
            collector_name,
            offset_value,
            updated_at: Utc::now(),
        }
    }
}
