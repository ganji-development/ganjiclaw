//! Notion sync pipeline.
//!
//! Syncs activity archive data to Notion databases.

use crate::schema::{NotionSyncItem, NotionSyncType, SyncStatus, Summary, Session};
use parking_lot::Mutex;
use rusqlite::{Connection, params};
use std::sync::Arc;
use std::time::Duration;
use anyhow::Result;
use chrono::Utc;

/// Notion sync manager.
#[allow(dead_code)]
pub struct NotionSync {
    db: Arc<Mutex<Connection>>,
    api_key: String,
    daily_logs_database_id: String,
    sessions_database_id: String,
    projects_database_id: String,
    queue_processor_interval: Duration,
}

impl NotionSync {
    /// Create a new Notion sync manager.
    ///
    /// # Arguments
    ///
    /// * `db` - Database connection
    /// * `api_key` - Notion API key
    /// * `daily_logs_database_id` - Notion database ID for daily logs
    /// * `sessions_database_id` - Notion database ID for sessions
    /// * `projects_database_id` - Notion database ID for projects
    /// * `queue_processor_interval` - How often to process the sync queue
    pub fn new(
        db: Arc<Mutex<Connection>>,
        api_key: String,
        daily_logs_database_id: String,
        sessions_database_id: String,
        projects_database_id: String,
        queue_processor_interval: Duration,
    ) -> Self {
        Self {
            db,
            api_key,
            daily_logs_database_id,
            sessions_database_id,
            projects_database_id,
            queue_processor_interval,
        }
    }

    /// Queue a daily log for sync.
    pub fn queue_daily_log(&self, summary: &Summary) -> Result<()> {
        let payload = serde_json::json!({
            "date": summary.period_start.format("%Y-%m-%d").to_string(),
            "summary": summary.content,
            "metrics": summary.metrics,
        });

        let item = NotionSyncItem::new(NotionSyncType::DailyLog, self.daily_logs_database_id.clone(), payload);
        self.store_sync_item(&item)?;

        Ok(())
    }

    /// Queue a session for sync.
    pub fn queue_session(&self, session: &Session) -> Result<()> {
        let payload = serde_json::json!({
            "start_time": session.start_ts_utc.to_rfc3339(),
            "end_time": session.end_ts_utc.map(|t| t.to_rfc3339()),
            "label": session.label.as_str(),
            "project_key": session.project_key,
            "tags": session.tags,
            "summary": session.summary,
            "event_count": session.event_count,
        });

        let item = NotionSyncItem::new(NotionSyncType::Session, self.sessions_database_id.clone(), payload);
        self.store_sync_item(&item)?;

        Ok(())
    }

    /// Queue a project for sync.
    pub fn queue_project(&self, project_key: &str, summary: &Summary) -> Result<()> {
        let payload = serde_json::json!({
            "name": project_key,
            "summary": summary.content,
            "metrics": summary.metrics,
            "last_activity": summary.period_end.to_rfc3339(),
        });

        let item = NotionSyncItem::new(NotionSyncType::Project, self.projects_database_id.clone(), payload);
        self.store_sync_item(&item)?;

        Ok(())
    }

    /// Process pending sync queue items.
    pub async fn process_queue(&self) -> Result<()> {
        loop {
            // Fetch pending items
            let pending_items = self.get_pending_items(10)?;

            if pending_items.is_empty() {
                tokio::time::sleep(self.queue_processor_interval).await;
                continue;
            }

            // Process each item
            for item in pending_items {
                self.process_item(&item).await?;
            }

            // Sleep before next batch
            tokio::time::sleep(self.queue_processor_interval).await;
        }
    }

    /// Get pending sync items.
    fn get_pending_items(&self, limit: usize) -> Result<Vec<NotionSyncItem>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare(
            "SELECT id, sync_type, target_id, payload_json, status, notion_page_id, error_message, created_at, updated_at, retry_count
             FROM notion_sync_queue
             WHERE status = 'pending'
             ORDER BY created_at ASC
             LIMIT ?1"
        )?;

        let items = stmt.query_map(params![limit], |row| {
            Ok(NotionSyncItem {
                id: row.get(0)?,
                sync_type: NotionSyncType::from_str(&row.get::<_, String>(1)?)
                    .unwrap_or(NotionSyncType::DailyLog),
                target_id: row.get(2)?,
                payload: serde_json::from_str(&row.get::<_, String>(3)?).unwrap_or_default(),
                status: SyncStatus::from_str(&row.get::<_, String>(4)?)
                    .unwrap_or(SyncStatus::Pending),
                notion_page_id: row.get(5)?,
                error_message: row.get(6)?,
                created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(7)?)
                    .unwrap()
                    .with_timezone(&Utc),
                updated_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(8)?)
                    .unwrap()
                    .with_timezone(&Utc),
                retry_count: row.get(9)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;

        Ok(items)
    }

    /// Process a single sync item.
    async fn process_item(&self, item: &NotionSyncItem) -> Result<()> {
        // Update status to syncing
        self.update_item_status(&item.id, SyncStatus::Syncing, None)?;

        // Call Notion API
        let result = match item.sync_type {
            NotionSyncType::DailyLog => self.sync_daily_log_to_notion(item).await,
            NotionSyncType::Session => self.sync_session_to_notion(item).await,
            NotionSyncType::Project => self.sync_project_to_notion(item).await,
            NotionSyncType::Pattern | NotionSyncType::Decision => {
                // Not implemented yet
                Ok(None)
            }
        };

        match result {
            Ok(page_id) => {
                // Update status to synced
                self.update_item_status(&item.id, SyncStatus::Synced, page_id)?;
            }
            Err(e) => {
                // Update status to failed
                self.update_item_status(&item.id, SyncStatus::Failed, None)?;
                self.increment_item_retry_count(&item.id)?;
                tracing::error!("Failed to sync item {}: {}", item.id, e);
            }
        }

        Ok(())
    }

    /// Sync a daily log to Notion.
    async fn sync_daily_log_to_notion(&self, item: &NotionSyncItem) -> Result<Option<String>> {
        let date = item.payload.get("date").and_then(|v| v.as_str()).unwrap_or("unknown");
        let summary = item.payload.get("summary").and_then(|v| v.as_str()).unwrap_or("");
        let metrics = &item.payload["metrics"];

        let body = serde_json::json!({
            "parent": { "database_id": self.daily_logs_database_id },
            "properties": {
                "Date": { "date": { "start": date } },
                "Summary": { "title": [{ "text": { "content": &summary[..summary.len().min(2000)] } }] },
                "Total Events": { "number": metrics.get("total_events").and_then(|v| v.as_f64()).unwrap_or(0.0) },
            },
            "children": [{
                "object": "block",
                "type": "paragraph",
                "paragraph": {
                    "rich_text": [{ "text": { "content": summary } }]
                }
            }]
        });

        self.notion_api_create_page(body).await
    }

    /// Sync a session to Notion.
    async fn sync_session_to_notion(&self, item: &NotionSyncItem) -> Result<Option<String>> {
        let start = item.payload.get("start_time").and_then(|v| v.as_str()).unwrap_or("");
        let end = item.payload.get("end_time").and_then(|v| v.as_str());
        let label = item.payload.get("label").and_then(|v| v.as_str()).unwrap_or("unlabeled");
        let project = item.payload.get("project_key").and_then(|v| v.as_str()).unwrap_or("");
        let event_count = item.payload.get("event_count").and_then(|v| v.as_f64()).unwrap_or(0.0);

        let body = serde_json::json!({
            "parent": { "database_id": self.sessions_database_id },
            "properties": {
                "Label": { "title": [{ "text": { "content": label } }] },
                "Start": { "date": { "start": start, "end": end } },
                "Project": { "rich_text": [{ "text": { "content": project } }] },
                "Events": { "number": event_count },
            }
        });

        self.notion_api_create_page(body).await
    }

    /// Sync a project to Notion.
    async fn sync_project_to_notion(&self, item: &NotionSyncItem) -> Result<Option<String>> {
        let name = item.payload.get("name").and_then(|v| v.as_str()).unwrap_or("unnamed");
        let summary = item.payload.get("summary").and_then(|v| v.as_str()).unwrap_or("");
        let last_activity = item.payload.get("last_activity").and_then(|v| v.as_str()).unwrap_or("");

        let body = serde_json::json!({
            "parent": { "database_id": self.projects_database_id },
            "properties": {
                "Name": { "title": [{ "text": { "content": name } }] },
                "Summary": { "rich_text": [{ "text": { "content": &summary[..summary.len().min(2000)] } }] },
                "Last Activity": { "date": { "start": last_activity } },
            }
        });

        self.notion_api_create_page(body).await
    }

    /// Create a page via the Notion API with retry and rate limiting.
    async fn notion_api_create_page(&self, body: serde_json::Value) -> Result<Option<String>> {
        const NOTION_API_URL: &str = "https://api.notion.com/v1/pages";
        const MAX_RETRIES: u32 = 3;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;

        for attempt in 0..=MAX_RETRIES {
            let response = client
                .post(NOTION_API_URL)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Notion-Version", "2022-06-28")
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await;

            match response {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        let json: serde_json::Value = resp.json().await?;
                        let page_id = json.get("id").and_then(|v| v.as_str()).map(|s| s.to_string());
                        // Rate limit: ~3 req/s
                        tokio::time::sleep(Duration::from_millis(334)).await;
                        return Ok(page_id);
                    }

                    if status.as_u16() == 429 || status.is_server_error() {
                        if attempt < MAX_RETRIES {
                            let backoff = Duration::from_secs(2u64.pow(attempt));
                            tracing::warn!(
                                "Notion API returned {}, retrying in {:?} (attempt {}/{})",
                                status, backoff, attempt + 1, MAX_RETRIES
                            );
                            tokio::time::sleep(backoff).await;
                            continue;
                        }
                    }

                    let body_text = resp.text().await.unwrap_or_default();
                    anyhow::bail!("Notion API error {status}: {body_text}");
                }
                Err(e) => {
                    if attempt < MAX_RETRIES {
                        let backoff = Duration::from_secs(2u64.pow(attempt));
                        tracing::warn!("Notion request failed: {e}, retrying in {:?}", backoff);
                        tokio::time::sleep(backoff).await;
                        continue;
                    }
                    anyhow::bail!("Notion request failed after {MAX_RETRIES} retries: {e}");
                }
            }
        }

        anyhow::bail!("Notion sync exhausted all retries")
    }

    /// Store a sync item in the database.
    fn store_sync_item(&self, item: &NotionSyncItem) -> Result<()> {
        let conn = self.db.lock();

        conn.execute(
            "INSERT INTO notion_sync_queue (
                id, sync_type, target_id, payload_json, status, notion_page_id, error_message, created_at, updated_at, retry_count
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                item.id,
                item.sync_type.as_str(),
                item.target_id,
                serde_json::to_string(&item.payload)?,
                item.status.as_str(),
                item.notion_page_id,
                item.error_message,
                item.created_at.to_rfc3339(),
                item.updated_at.to_rfc3339(),
                item.retry_count,
            ],
        )?;

        Ok(())
    }

    /// Update item status.
    fn update_item_status(&self, id: &str, status: SyncStatus, notion_page_id: Option<String>) -> Result<()> {
        let conn = self.db.lock();

        conn.execute(
            "UPDATE notion_sync_queue
             SET status = ?1, notion_page_id = ?2, updated_at = ?3
             WHERE id = ?4",
            params![
                status.as_str(),
                notion_page_id,
                Utc::now().to_rfc3339(),
                id,
            ],
        )?;

        Ok(())
    }

    /// Increment item retry count.
    fn increment_item_retry_count(&self, id: &str) -> Result<()> {
        let conn = self.db.lock();

        conn.execute(
            "UPDATE notion_sync_queue
             SET retry_count = retry_count + 1, updated_at = ?1
             WHERE id = ?2",
            params![Utc::now().to_rfc3339(), id],
        )?;

        Ok(())
    }

    /// Get sync statistics.
    pub fn get_sync_stats(&self) -> Result<SyncStats> {
        let conn = self.db.lock();

        let pending: i64 = conn.query_row(
            "SELECT COUNT(*) FROM notion_sync_queue WHERE status = 'pending'",
            [],
            |row| row.get(0),
        )?;

        let syncing: i64 = conn.query_row(
            "SELECT COUNT(*) FROM notion_sync_queue WHERE status = 'syncing'",
            [],
            |row| row.get(0),
        )?;

        let synced: i64 = conn.query_row(
            "SELECT COUNT(*) FROM notion_sync_queue WHERE status = 'synced'",
            [],
            |row| row.get(0),
        )?;

        let failed: i64 = conn.query_row(
            "SELECT COUNT(*) FROM notion_sync_queue WHERE status = 'failed'",
            [],
            |row| row.get(0),
        )?;

        Ok(SyncStats {
            pending: pending as u32,
            syncing: syncing as u32,
            synced: synced as u32,
            failed: failed as u32,
        })
    }
}

/// Sync statistics.
#[derive(Debug, Clone)]
pub struct SyncStats {
    pub pending: u32,
    pub syncing: u32,
    pub synced: u32,
    pub failed: u32,
}
