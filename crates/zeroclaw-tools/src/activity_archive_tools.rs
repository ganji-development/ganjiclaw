//! Agent-callable tools for querying and managing the Activity Archive.
//!
//! These tools open *read-only* connections to the activity archive database
//! and are independent of the daemon's collector pipeline. They are registered
//! in `all_tools_with_runtime` when `config.activity_archive.enabled` is true.

use async_trait::async_trait;
use serde_json::{Value, json};
use std::path::PathBuf;
use zeroclaw_api::tool::{Tool, ToolResult};

// ── Helpers ──────────────────────────────────────────────────────────────────

fn open_readonly(db_path: &std::path::Path) -> anyhow::Result<rusqlite::Connection> {
    let conn = rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    Ok(conn)
}

fn open_readwrite(db_path: &std::path::Path) -> anyhow::Result<rusqlite::Connection> {
    let conn = rusqlite::Connection::open(db_path)?;
    Ok(conn)
}

fn err_result(msg: impl Into<String>) -> anyhow::Result<ToolResult> {
    Ok(ToolResult { success: false, output: String::new(), error: Some(msg.into()) })
}

fn ok_result(output: impl Into<String>) -> anyhow::Result<ToolResult> {
    Ok(ToolResult { success: true, output: output.into(), error: None })
}

// ── ActivityArchiveViewTool ──────────────────────────────────────────────────

/// List recent events from the activity archive.
pub struct ActivityArchiveViewTool { db_path: PathBuf }

impl ActivityArchiveViewTool {
    pub fn new(db_path: PathBuf) -> Self { Self { db_path } }
}

#[async_trait]
impl Tool for ActivityArchiveViewTool {
    fn name(&self) -> &str { "activity_archive_view" }

    fn description(&self) -> &str {
        "View recent events from the desktop activity archive. \
         Returns event type, app, title, and timestamp."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "hours": { "type": "integer", "description": "Look-back window in hours (default: 1)", "default": 1 },
                "limit": { "type": "integer", "description": "Max events to return (default: 50, max: 200)", "default": 50 },
                "source": { "type": "string", "description": "Filter by source (window_focus, browser_history, shell_activity, file_activity, process_launch)" }
            }
        })
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let hours = args.get("hours").and_then(|v| v.as_i64()).unwrap_or(1).max(1);
        let limit = args.get("limit").and_then(|v| v.as_i64()).unwrap_or(50).clamp(1, 200);
        let source_filter = args.get("source").and_then(|v| v.as_str()).map(|s| s.to_string());

        let conn = match open_readonly(&self.db_path) {
            Ok(c) => c,
            Err(e) => return err_result(format!("Failed to open DB: {e}")),
        };

        let cutoff = chrono::Utc::now() - chrono::Duration::hours(hours);
        let query = if source_filter.is_some() {
            "SELECT ts_utc, source, event_type, app, title FROM events WHERE ts_utc > ?1 AND source = ?2 ORDER BY ts_utc DESC LIMIT ?3"
        } else {
            "SELECT ts_utc, source, event_type, app, title FROM events WHERE ts_utc > ?1 ORDER BY ts_utc DESC LIMIT ?2"
        };

        let mut stmt = conn.prepare(query)?;
        let rows: Vec<String> = if let Some(ref src) = source_filter {
            stmt.query_map(rusqlite::params![cutoff.to_rfc3339(), src, limit], |row| {
                let ts: String = row.get(0)?;
                let source: String = row.get(1)?;
                let etype: String = row.get(2)?;
                let app: String = row.get::<_, Option<String>>(3)?.unwrap_or_default();
                let title: String = row.get::<_, Option<String>>(4)?.unwrap_or_default();
                Ok(format!("[{ts}] {source}/{etype} | {app} | {title}"))
            })?.filter_map(|r| r.ok()).collect()
        } else {
            stmt.query_map(rusqlite::params![cutoff.to_rfc3339(), limit], |row| {
                let ts: String = row.get(0)?;
                let source: String = row.get(1)?;
                let etype: String = row.get(2)?;
                let app: String = row.get::<_, Option<String>>(3)?.unwrap_or_default();
                let title: String = row.get::<_, Option<String>>(4)?.unwrap_or_default();
                Ok(format!("[{ts}] {source}/{etype} | {app} | {title}"))
            })?.filter_map(|r| r.ok()).collect()
        };

        if rows.is_empty() {
            ok_result(format!("No events in the last {hours} hour(s)."))
        } else {
            ok_result(format!("{} events:\n{}", rows.len(), rows.join("\n")))
        }
    }
}

// ── ActivityArchiveStatsTool ─────────────────────────────────────────────────

/// Aggregate statistics from the activity archive.
pub struct ActivityArchiveStatsTool { db_path: PathBuf }
impl ActivityArchiveStatsTool { pub fn new(db_path: PathBuf) -> Self { Self { db_path } } }

#[async_trait]
impl Tool for ActivityArchiveStatsTool {
    fn name(&self) -> &str { "activity_archive_stats" }
    fn description(&self) -> &str {
        "Get aggregate statistics: top apps, event counts, session count, and active time."
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "hours": { "type": "integer", "description": "Look-back window (default: 24)", "default": 24 }
            }
        })
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let hours = args.get("hours").and_then(|v| v.as_i64()).unwrap_or(24).max(1);
        let conn = match open_readonly(&self.db_path) {
            Ok(c) => c,
            Err(e) => return err_result(format!("Failed to open DB: {e}")),
        };
        let cutoff = chrono::Utc::now() - chrono::Duration::hours(hours);

        let total: i64 = conn.query_row(
            "SELECT COUNT(*) FROM events WHERE ts_utc > ?1",
            rusqlite::params![cutoff.to_rfc3339()], |r| r.get(0),
        ).unwrap_or(0);

        let session_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sessions WHERE start_ts_utc > ?1",
            rusqlite::params![cutoff.to_rfc3339()], |r| r.get(0),
        ).unwrap_or(0);

        let mut top_apps_stmt = conn.prepare(
            "SELECT app, COUNT(*) as cnt FROM events WHERE ts_utc > ?1 AND app IS NOT NULL
             GROUP BY app ORDER BY cnt DESC LIMIT 10"
        )?;
        let top_apps: Vec<String> = top_apps_stmt.query_map(
            rusqlite::params![cutoff.to_rfc3339()],
            |row| { Ok(format!("  {} ({})", row.get::<_, String>(0)?, row.get::<_, i64>(1)?)) },
        )?.filter_map(|r| r.ok()).collect();

        let mut source_stmt = conn.prepare(
            "SELECT source, COUNT(*) as cnt FROM events WHERE ts_utc > ?1
             GROUP BY source ORDER BY cnt DESC"
        )?;
        let sources: Vec<String> = source_stmt.query_map(
            rusqlite::params![cutoff.to_rfc3339()],
            |row| { Ok(format!("  {} ({})", row.get::<_, String>(0)?, row.get::<_, i64>(1)?)) },
        )?.filter_map(|r| r.ok()).collect();

        let output = format!(
            "Activity stats (last {hours}h):\n\
             Total events: {total}\n\
             Sessions: {session_count}\n\n\
             Top apps:\n{}\n\n\
             By source:\n{}",
            top_apps.join("\n"),
            sources.join("\n"),
        );
        ok_result(output)
    }
}

// ── ActivityArchiveSearchTool ────────────────────────────────────────────────

/// Full-text search over event titles, URLs, commands.
pub struct ActivityArchiveSearchTool { db_path: PathBuf }
impl ActivityArchiveSearchTool { pub fn new(db_path: PathBuf) -> Self { Self { db_path } } }

#[async_trait]
impl Tool for ActivityArchiveSearchTool {
    fn name(&self) -> &str { "activity_archive_search" }
    fn description(&self) -> &str {
        "Search activity archive events by keyword in title, app, or details."
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search term" },
                "hours": { "type": "integer", "description": "Look-back (default: 24)", "default": 24 },
                "limit": { "type": "integer", "description": "Max results (default: 30)", "default": 30 }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let query = match args.get("query").and_then(|v| v.as_str()) {
            Some(q) if !q.trim().is_empty() => q.trim(),
            _ => return err_result("Missing 'query' parameter"),
        };
        let hours = args.get("hours").and_then(|v| v.as_i64()).unwrap_or(24).max(1);
        let limit = args.get("limit").and_then(|v| v.as_i64()).unwrap_or(30).clamp(1, 100);

        let conn = match open_readonly(&self.db_path) {
            Ok(c) => c, Err(e) => return err_result(format!("DB: {e}")),
        };
        let cutoff = chrono::Utc::now() - chrono::Duration::hours(hours);
        let like = format!("%{query}%");

        let mut stmt = conn.prepare(
            "SELECT ts_utc, source, app, title FROM events
             WHERE ts_utc > ?1 AND (title LIKE ?2 OR app LIKE ?2 OR details_json LIKE ?2)
             ORDER BY ts_utc DESC LIMIT ?3"
        )?;
        let rows: Vec<String> = stmt.query_map(
            rusqlite::params![cutoff.to_rfc3339(), like, limit],
            |row| {
                let ts: String = row.get(0)?;
                let src: String = row.get(1)?;
                let app: String = row.get::<_, Option<String>>(2)?.unwrap_or_default();
                let title: String = row.get::<_, Option<String>>(3)?.unwrap_or_default();
                Ok(format!("[{ts}] {src} | {app} | {title}"))
            },
        )?.filter_map(|r| r.ok()).collect();

        if rows.is_empty() {
            ok_result(format!("No events matching '{query}' in the last {hours}h."))
        } else {
            ok_result(format!("{} results for '{query}':\n{}", rows.len(), rows.join("\n")))
        }
    }
}

// ── ActivityArchiveListSessionsTool ──────────────────────────────────────────

pub struct ActivityArchiveListSessionsTool { db_path: PathBuf }
impl ActivityArchiveListSessionsTool { pub fn new(db_path: PathBuf) -> Self { Self { db_path } } }

#[async_trait]
impl Tool for ActivityArchiveListSessionsTool {
    fn name(&self) -> &str { "activity_archive_sessions" }
    fn description(&self) -> &str { "List inferred activity sessions with duration and dominant app." }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "hours": { "type": "integer", "description": "Look-back (default: 24)", "default": 24 },
                "limit": { "type": "integer", "default": 20 }
            }
        })
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let hours = args.get("hours").and_then(|v| v.as_i64()).unwrap_or(24).max(1);
        let limit = args.get("limit").and_then(|v| v.as_i64()).unwrap_or(20).clamp(1, 50);
        let conn = match open_readonly(&self.db_path) {
            Ok(c) => c, Err(e) => return err_result(format!("DB: {e}")),
        };
        let cutoff = chrono::Utc::now() - chrono::Duration::hours(hours);

        let mut stmt = conn.prepare(
            "SELECT id, start_ts_utc, end_ts_utc, dominant_app, project_key, event_count
             FROM sessions WHERE start_ts_utc > ?1 ORDER BY start_ts_utc DESC LIMIT ?2"
        )?;
        let rows: Vec<String> = stmt.query_map(
            rusqlite::params![cutoff.to_rfc3339(), limit],
            |row| {
                let start: String = row.get(1)?;
                let end: String = row.get::<_, Option<String>>(2)?.unwrap_or_else(|| "ongoing".into());
                let app: String = row.get::<_, Option<String>>(3)?.unwrap_or_default();
                let proj: String = row.get::<_, Option<String>>(4)?.unwrap_or_default();
                let count: i64 = row.get::<_, Option<i64>>(5)?.unwrap_or(0);
                Ok(format!("{start} → {end} | {app} | {proj} ({count} events)"))
            },
        )?.filter_map(|r| r.ok()).collect();

        if rows.is_empty() {
            ok_result("No sessions found.")
        } else {
            ok_result(format!("{} sessions:\n{}", rows.len(), rows.join("\n")))
        }
    }
}

// ── ActivityArchiveSummarizeTool ─────────────────────────────────────────────

pub struct ActivityArchiveSummarizeTool { db_path: PathBuf }
impl ActivityArchiveSummarizeTool { pub fn new(db_path: PathBuf) -> Self { Self { db_path } } }

#[async_trait]
impl Tool for ActivityArchiveSummarizeTool {
    fn name(&self) -> &str { "activity_archive_summarize" }
    fn description(&self) -> &str { "Get pre-generated hourly or daily summaries." }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "type": { "type": "string", "enum": ["hourly", "daily"], "default": "daily" },
                "date": { "type": "string", "description": "Date in YYYY-MM-DD format (default: today)" },
                "limit": { "type": "integer", "default": 5 }
            }
        })
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let stype = args.get("type").and_then(|v| v.as_str()).unwrap_or("daily");
        let limit = args.get("limit").and_then(|v| v.as_i64()).unwrap_or(5).clamp(1, 20);
        let conn = match open_readonly(&self.db_path) {
            Ok(c) => c, Err(e) => return err_result(format!("DB: {e}")),
        };

        let mut stmt = conn.prepare(
            "SELECT summary_type, period_start, content FROM summaries
             WHERE summary_type = ?1 ORDER BY period_start DESC LIMIT ?2"
        )?;
        let rows: Vec<String> = stmt.query_map(
            rusqlite::params![stype, limit],
            |row| {
                let period: String = row.get(1)?;
                let content: String = row.get(2)?;
                Ok(format!("── {period} ──\n{content}"))
            },
        )?.filter_map(|r| r.ok()).collect();

        if rows.is_empty() {
            ok_result(format!("No {stype} summaries found."))
        } else {
            ok_result(rows.join("\n\n"))
        }
    }
}

// ── Privacy Tools ────────────────────────────────────────────────────────────

pub struct ActivityArchivePrivacyListTool { db_path: PathBuf }
impl ActivityArchivePrivacyListTool { pub fn new(db_path: PathBuf) -> Self { Self { db_path } } }

#[async_trait]
impl Tool for ActivityArchivePrivacyListTool {
    fn name(&self) -> &str { "activity_archive_privacy_list" }
    fn description(&self) -> &str { "List active privacy rules for the activity archive." }
    fn parameters_schema(&self) -> Value { json!({ "type": "object", "properties": {} }) }

    async fn execute(&self, _args: Value) -> anyhow::Result<ToolResult> {
        let conn = match open_readonly(&self.db_path) {
            Ok(c) => c, Err(e) => return err_result(format!("DB: {e}")),
        };
        let mut stmt = conn.prepare("SELECT id, rule_type, pattern, action FROM privacy_rules")?;
        let rows: Vec<String> = stmt.query_map([], |row| {
            Ok(format!("  [{}] {} '{}' → {}",
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?.filter_map(|r| r.ok()).collect();

        if rows.is_empty() {
            ok_result("No privacy rules configured.")
        } else {
            ok_result(format!("{} rules:\n{}", rows.len(), rows.join("\n")))
        }
    }
}

pub struct ActivityArchivePrivacyAddTool { db_path: PathBuf }
impl ActivityArchivePrivacyAddTool { pub fn new(db_path: PathBuf) -> Self { Self { db_path } } }

#[async_trait]
impl Tool for ActivityArchivePrivacyAddTool {
    fn name(&self) -> &str { "activity_archive_privacy_add" }
    fn description(&self) -> &str {
        "Add a privacy exclusion rule. Types: exclude_path, exclude_title, exclude_domain."
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "rule_type": { "type": "string", "enum": ["exclude_path", "exclude_title", "exclude_domain"] },
                "pattern": { "type": "string", "description": "Glob pattern to exclude" }
            },
            "required": ["rule_type", "pattern"]
        })
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let rule_type = match args.get("rule_type").and_then(|v| v.as_str()) {
            Some(t) => t, _ => return err_result("Missing 'rule_type'"),
        };
        let pattern = match args.get("pattern").and_then(|v| v.as_str()) {
            Some(p) if !p.trim().is_empty() => p.trim(),
            _ => return err_result("Missing 'pattern'"),
        };

        let conn = match open_readwrite(&self.db_path) {
            Ok(c) => c, Err(e) => return err_result(format!("DB: {e}")),
        };
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO privacy_rules (id, rule_type, pattern, action, created_at) VALUES (?1, ?2, ?3, 'exclude', ?4)",
            rusqlite::params![id, rule_type, pattern, now],
        )?;
        ok_result(format!("Added rule {id}: {rule_type} '{pattern}'"))
    }
}

pub struct ActivityArchivePrivacyRemoveTool { db_path: PathBuf }
impl ActivityArchivePrivacyRemoveTool { pub fn new(db_path: PathBuf) -> Self { Self { db_path } } }

#[async_trait]
impl Tool for ActivityArchivePrivacyRemoveTool {
    fn name(&self) -> &str { "activity_archive_privacy_remove" }
    fn description(&self) -> &str { "Remove a privacy rule by its ID." }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Rule ID to remove" }
            },
            "required": ["id"]
        })
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let id = match args.get("id").and_then(|v| v.as_str()) {
            Some(i) => i, _ => return err_result("Missing 'id'"),
        };
        let conn = match open_readwrite(&self.db_path) {
            Ok(c) => c, Err(e) => return err_result(format!("DB: {e}")),
        };
        let deleted = conn.execute("DELETE FROM privacy_rules WHERE id = ?1", rusqlite::params![id])?;
        if deleted > 0 {
            ok_result(format!("Removed privacy rule {id}."))
        } else {
            err_result(format!("No rule found with ID '{id}'."))
        }
    }
}

// ── Notion Sync Trigger ──────────────────────────────────────────────────────

pub struct ActivityArchiveSyncNotionTool { db_path: PathBuf }
impl ActivityArchiveSyncNotionTool { pub fn new(db_path: PathBuf) -> Self { Self { db_path } } }

#[async_trait]
impl Tool for ActivityArchiveSyncNotionTool {
    fn name(&self) -> &str { "activity_archive_sync_notion" }
    fn description(&self) -> &str { "Trigger a Notion sync for recent summaries/sessions." }
    fn parameters_schema(&self) -> Value {
        json!({ "type": "object", "properties": {
            "type": { "type": "string", "enum": ["daily_log", "session", "project"], "default": "daily_log" }
        }})
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let sync_type = args.get("type").and_then(|v| v.as_str()).unwrap_or("daily_log");
        let conn = match open_readwrite(&self.db_path) {
            Ok(c) => c, Err(e) => return err_result(format!("DB: {e}")),
        };
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO notion_sync_queue (id, entity_type, entity_id, action, status, queued_at)
             VALUES (?1, ?2, ?3, 'create', 'pending', ?4)",
            rusqlite::params![id, sync_type, "", now],
        )?;
        ok_result(format!("Queued {sync_type} sync (job {id}). Daemon will process shortly."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_names_unique() {
        let tools: Vec<Box<dyn Tool>> = vec![
            Box::new(ActivityArchiveViewTool::new(PathBuf::from("test.db"))),
            Box::new(ActivityArchiveStatsTool::new(PathBuf::from("test.db"))),
            Box::new(ActivityArchiveSearchTool::new(PathBuf::from("test.db"))),
            Box::new(ActivityArchiveListSessionsTool::new(PathBuf::from("test.db"))),
            Box::new(ActivityArchiveSummarizeTool::new(PathBuf::from("test.db"))),
            Box::new(ActivityArchivePrivacyListTool::new(PathBuf::from("test.db"))),
            Box::new(ActivityArchivePrivacyAddTool::new(PathBuf::from("test.db"))),
            Box::new(ActivityArchivePrivacyRemoveTool::new(PathBuf::from("test.db"))),
            Box::new(ActivityArchiveSyncNotionTool::new(PathBuf::from("test.db"))),
        ];
        let mut names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        let count = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), count, "tool names must be unique");
    }

    #[test]
    fn test_schemas_are_valid() {
        let tools: Vec<Box<dyn Tool>> = vec![
            Box::new(ActivityArchiveViewTool::new(PathBuf::from("test.db"))),
            Box::new(ActivityArchiveSearchTool::new(PathBuf::from("test.db"))),
            Box::new(ActivityArchivePrivacyAddTool::new(PathBuf::from("test.db"))),
        ];
        for tool in &tools {
            let schema = tool.parameters_schema();
            assert_eq!(schema["type"], "object", "schema for {} is not an object", tool.name());
        }
    }
}
