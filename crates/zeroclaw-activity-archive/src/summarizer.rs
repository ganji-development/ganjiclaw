//! Summarization of activity data.
//!
//! Generates hourly, daily, and project summaries using LLM.

use crate::schema::{Summary, SummaryType};
use chrono::Timelike;
use parking_lot::Mutex;
use rusqlite::{Connection, params};
use std::sync::Arc;
use std::collections::HashMap;
use anyhow::Result;
use chrono::{DateTime, NaiveDate, Utc};

/// Summarizer for generating activity summaries.
pub struct Summarizer {
    db: Arc<Mutex<Connection>>,
}

impl Summarizer {
    /// Create a new summarizer.
    pub fn new(db: Arc<Mutex<Connection>>) -> Self {
        Self { db }
    }

    /// Generate an hourly summary.
    pub fn generate_hourly_summary(&self, hour: DateTime<Utc>) -> Result<Summary> {
        let hour_start = hour;
        let hour_end = hour + chrono::Duration::hours(1);

        // Query events for the hour
        let events = self.get_events_in_range(hour_start, hour_end)?;

        // Generate summary content
        let content = self.generate_summary_content(&events, "hourly");

        // Calculate metrics
        let metrics = self.calculate_metrics(&events);

        let mut summary = Summary::new(SummaryType::Hourly, hour_start, hour_end);
        summary.content = content;
        summary.metrics = metrics;

        // Store in database
        self.store_summary(&summary)?;

        Ok(summary)
    }

    /// Generate a daily log.
    pub fn generate_daily_log(&self, date: NaiveDate) -> Result<Summary> {
        let date_start = date.and_hms_opt(0, 0, 0).unwrap().and_utc();
        let date_end = (date + chrono::Duration::days(1)).and_hms_opt(0, 0, 0).unwrap().and_utc();

        // Query events for the day
        let events = self.get_events_in_range(date_start, date_end)?;

        // Generate daily log content
        let content = self.generate_daily_log_content(&events);

        // Calculate metrics
        let metrics = self.calculate_metrics(&events);

        let mut summary = Summary::new(SummaryType::Daily, date_start, date_end);
        summary.content = content;
        summary.metrics = metrics;

        // Store in database
        self.store_summary(&summary)?;

        Ok(summary)
    }

    /// Generate a project summary.
    pub fn generate_project_summary(
        &self,
        project_key: &str,
        period_start: DateTime<Utc>,
        period_end: DateTime<Utc>,
    ) -> Result<Summary> {
        // Query events for the project in the period
        let events = self.get_project_events_in_range(project_key, period_start, period_end)?;

        // Generate project summary content
        let content = self.generate_project_summary_content(project_key, &events);

        // Calculate metrics
        let metrics = self.calculate_metrics(&events);

        let mut summary = Summary::new(SummaryType::Project, period_start, period_end);
        summary.project_key = Some(project_key.to_string());
        summary.content = content;
        summary.metrics = metrics;

        // Store in database
        self.store_summary(&summary)?;

        Ok(summary)
    }

    /// Get events in a time range.
    fn get_events_in_range(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Result<Vec<crate::schema::Event>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare(
            "SELECT id, ts_utc, ts_local, source, event_type, actor, host, app, title, path,
                    details_json, sensitivity, project_key, session_id, hash, raw_ref, created_at
             FROM events
             WHERE ts_utc >= ?1 AND ts_utc < ?2
             ORDER BY ts_utc ASC"
        )?;

        let events = stmt.query_map(params![start.to_rfc3339(), end.to_rfc3339()], |row| {
            Ok(crate::schema::Event {
                id: row.get(0)?,
                ts_utc: DateTime::parse_from_rfc3339(&row.get::<_, String>(1)?)
                    .unwrap()
                    .with_timezone(&Utc),
                ts_local: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(2)?)
                    .unwrap()
                    .with_timezone(&chrono::Local),
                source: row.get(3)?,
                event_type: crate::schema::EventType::from_str(&row.get::<_, String>(4)?)
                    .unwrap_or(crate::schema::EventType::SystemEvent),
                actor: row.get(5)?,
                host: row.get(6)?,
                app: row.get(7)?,
                title: row.get(8)?,
                path: row.get(9)?,
                details: serde_json::from_str(&row.get::<_, String>(10)?).unwrap_or_default(),
                sensitivity: row.get(11)?,
                project_key: row.get(12)?,
                session_id: row.get(13)?,
                hash: row.get(14)?,
                raw_ref: row.get(15)?,
                created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(16)?)
                    .unwrap()
                    .with_timezone(&Utc),
            })
        })?.collect::<Result<Vec<_>, _>>()?;

        Ok(events)
    }

    /// Get project events in a time range.
    fn get_project_events_in_range(
        &self,
        project_key: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<crate::schema::Event>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare(
            "SELECT id, ts_utc, ts_local, source, event_type, actor, host, app, title, path,
                    details_json, sensitivity, project_key, session_id, hash, raw_ref, created_at
             FROM events
             WHERE project_key = ?1 AND ts_utc >= ?2 AND ts_utc < ?3
             ORDER BY ts_utc ASC"
        )?;

        let events = stmt.query_map(params![project_key, start.to_rfc3339(), end.to_rfc3339()], |row| {
            Ok(crate::schema::Event {
                id: row.get(0)?,
                ts_utc: DateTime::parse_from_rfc3339(&row.get::<_, String>(1)?)
                    .unwrap()
                    .with_timezone(&Utc),
                ts_local: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(2)?)
                    .unwrap()
                    .with_timezone(&chrono::Local),
                source: row.get(3)?,
                event_type: crate::schema::EventType::from_str(&row.get::<_, String>(4)?)
                    .unwrap_or(crate::schema::EventType::SystemEvent),
                actor: row.get(5)?,
                host: row.get(6)?,
                app: row.get(7)?,
                title: row.get(8)?,
                path: row.get(9)?,
                details: serde_json::from_str(&row.get::<_, String>(10)?).unwrap_or_default(),
                sensitivity: row.get(11)?,
                project_key: row.get(12)?,
                session_id: row.get(13)?,
                hash: row.get(14)?,
                raw_ref: row.get(15)?,
                created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(16)?)
                    .unwrap()
                    .with_timezone(&Utc),
            })
        })?.collect::<Result<Vec<_>, _>>()?;

        Ok(events)
    }

    /// Generate summary content from events.
    fn generate_summary_content(&self, events: &[crate::schema::Event], summary_type: &str) -> String {
        if events.is_empty() {
            return format!("No activity recorded for this {}.", summary_type);
        }

        // Count events by type
        let mut event_counts = std::collections::HashMap::new();
        for event in events {
            *event_counts.entry(event.event_type.as_str()).or_insert(0) += 1;
        }

        // Build summary
        let mut content = format!("Activity summary for {}:\n\n", summary_type);
        content.push_str(&format!("Total events: {}\n\n", events.len()));

        content.push_str("Event breakdown:\n");
        for (event_type, count) in event_counts.iter() {
            content.push_str(&format!("  - {}: {}\n", event_type, count));
        }

        // Top apps
        let mut app_counts = std::collections::HashMap::new();
        for event in events {
            if let Some(app) = &event.app {
                *app_counts.entry(app.as_str()).or_insert(0) += 1;
            }
        }

        if !app_counts.is_empty() {
            content.push_str("\nTop applications:\n");
            let mut sorted_apps: Vec<_> = app_counts.iter().collect();
            sorted_apps.sort_by(|a, b| b.1.cmp(a.1));

            for (app, count) in sorted_apps.iter().take(5) {
                content.push_str(&format!("  - {}: {}\n", app, count));
            }
        }

        content
    }

    /// Generate daily log content from events.
    fn generate_daily_log_content(&self, events: &[crate::schema::Event]) -> String {
        if events.is_empty() {
            return "No activity recorded today.".to_string();
        }

        let mut content = String::new();

        // Time-based summary
        let first_event = events.first().unwrap();
        let last_event = events.last().unwrap();

        content.push_str(&format!(
            "Activity from {} to {}\n\n",
            first_event.ts_local.format("%H:%M"),
            last_event.ts_local.format("%H:%M")
        ));

        // Top projects
        let mut project_counts = std::collections::HashMap::new();
        for event in events {
            if let Some(project) = &event.project_key {
                *project_counts.entry(project.as_str()).or_insert(0) += 1;
            }
        }

        if !project_counts.is_empty() {
            content.push_str("Top projects:\n");
            let mut sorted_projects: Vec<_> = project_counts.iter().collect();
            sorted_projects.sort_by(|a, b| b.1.cmp(a.1));

            for (project, count) in sorted_projects.iter().take(5) {
                content.push_str(&format!("  - {}: {} events\n", project, count));
            }
            content.push('\n');
        }

        // Important moments (simplified - would use LLM in production)
        content.push_str("Notable activity:\n");
        for event in events.iter().take(10) {
            if let Some(title) = &event.title {
                if !title.is_empty() {
                    content.push_str(&format!(
                        "  - [{}] {}: {}\n",
                        event.ts_local.format("%H:%M"),
                        event.app.as_deref().unwrap_or("unknown"),
                        title
                    ));
                }
            }
        }

        content
    }

    /// Generate project summary content.
    fn generate_project_summary_content(&self, project_key: &str, events: &[crate::schema::Event]) -> String {
        if events.is_empty() {
            return format!("No activity recorded for project {}.", project_key);
        }

        let mut content = format!("Project summary for {}\n\n", project_key);
        content.push_str(&format!("Total events: {}\n\n", events.len()));

        // Time distribution
        let mut hourly_counts = [0u32; 24];
        for event in events {
            let hour = event.ts_local.hour() as usize;
            if hour < 24 {
                hourly_counts[hour] += 1;
            }
        }

        content.push_str("Activity by hour:\n");
        for (hour, count) in hourly_counts.iter().enumerate() {
            if *count > 0 {
                content.push_str(&format!("  - {}:00: {} events\n", hour, count));
            }
        }

        content
    }

    /// Calculate metrics from events.
    fn calculate_metrics(&self, events: &[crate::schema::Event]) -> serde_json::Value {
        let mut metrics = serde_json::Map::new();

        metrics.insert("total_events".to_string(), serde_json::Value::Number(events.len().into()));

        // Count by event type
        let mut event_type_counts: HashMap<&str, u32> = HashMap::new();
        for event in events {
            let key = event.event_type.as_str();
            *event_type_counts.entry(key).or_insert(0) += 1;
        }
        let event_type_counts_json: serde_json::Map<String, serde_json::Value> = event_type_counts
            .into_iter()
            .map(|(k, v)| (k.to_string(), serde_json::Value::Number(v.into())))
            .collect();
        metrics.insert("event_types".to_string(), serde_json::Value::Object(event_type_counts_json));

        // Count by app
        let mut app_counts: HashMap<&str, u32> = HashMap::new();
        for event in events {
            if let Some(app) = &event.app {
                *app_counts.entry(app.as_str()).or_insert(0) += 1;
            }
        }
        let app_counts_json: serde_json::Map<String, serde_json::Value> = app_counts
            .into_iter()
            .map(|(k, v)| (k.to_string(), serde_json::Value::Number(v.into())))
            .collect();
        metrics.insert("apps".to_string(), serde_json::Value::Object(app_counts_json));

        // Count by project
        let mut project_counts: HashMap<&str, u32> = HashMap::new();
        for event in events {
            if let Some(project) = &event.project_key {
                *project_counts.entry(project.as_str()).or_insert(0) += 1;
            }
        }
        let project_counts_json: serde_json::Map<String, serde_json::Value> = project_counts
            .into_iter()
            .map(|(k, v)| (k.to_string(), serde_json::Value::Number(v.into())))
            .collect();
        metrics.insert("projects".to_string(), serde_json::Value::Object(project_counts_json));

        serde_json::Value::Object(metrics)
    }

    /// Store a summary in the database.
    fn store_summary(&self, summary: &Summary) -> Result<()> {
        let conn = self.db.lock();

        conn.execute(
            "INSERT INTO summaries (
                id, summary_type, period_start, period_end, project_key, topic, content, metrics_json, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                summary.id,
                summary.summary_type.as_str(),
                summary.period_start.to_rfc3339(),
                summary.period_end.to_rfc3339(),
                summary.project_key,
                summary.topic,
                summary.content,
                serde_json::to_string(&summary.metrics)?,
                summary.created_at.to_rfc3339(),
            ],
        )?;

        Ok(())
    }

    /// Get existing summary for a period.
    pub fn get_summary(&self, summary_type: SummaryType, period_start: DateTime<Utc>, period_end: DateTime<Utc>) -> Result<Option<Summary>> {
        let conn = self.db.lock();

        let mut stmt = conn.prepare(
            "SELECT id, summary_type, period_start, period_end, project_key, topic, content, metrics_json, created_at
             FROM summaries
             WHERE summary_type = ?1 AND period_start = ?2 AND period_end = ?3"
        )?;

        let result = stmt.query_row(
            params![summary_type.as_str(), period_start.to_rfc3339(), period_end.to_rfc3339()],
            |row| {
                Ok(Summary {
                    id: row.get(0)?,
                    summary_type: SummaryType::from_str(&row.get::<_, String>(1)?)
                        .unwrap_or(SummaryType::Daily),
                    period_start: DateTime::parse_from_rfc3339(&row.get::<_, String>(2)?)
                        .unwrap()
                        .with_timezone(&Utc),
                    period_end: DateTime::parse_from_rfc3339(&row.get::<_, String>(3)?)
                        .unwrap()
                        .with_timezone(&Utc),
                    project_key: row.get(4)?,
                    topic: row.get(5)?,
                    content: row.get(6)?,
                    metrics: serde_json::from_str(&row.get::<_, String>(7)?).unwrap_or_default(),
                    created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(8)?)
                        .unwrap()
                        .with_timezone(&Utc),
                })
            },
        );

        match result {
            Ok(summary) => Ok(Some(summary)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{init_schema, Event, EventType};
    use parking_lot::Mutex;
    use std::sync::Arc;

    fn setup_db() -> Arc<Mutex<rusqlite::Connection>> {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        Arc::new(Mutex::new(conn))
    }

    fn insert_event(db: &Arc<Mutex<rusqlite::Connection>>, ts: DateTime<Utc>, app: &str) {
        let id = uuid::Uuid::new_v4().to_string();
        let conn = db.lock();
        conn.execute(
            "INSERT INTO events (id, ts_utc, ts_local, source, event_type, app, title, details_json, sensitivity, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                id, ts.to_rfc3339(), chrono::Local::now().to_rfc3339(),
                "window_focus", "window_focus", app,
                format!("{} - title", app), "{}", 0, Utc::now().to_rfc3339(),
            ],
        ).unwrap();
    }

    #[test]
    fn test_hourly_summary_content() {
        let db = setup_db();
        let hour = Utc::now() - chrono::Duration::hours(1);

        // Insert events within the hour
        for i in 0..5 {
            insert_event(&db, hour + chrono::Duration::minutes(i * 10), "Code.exe");
        }
        for i in 0..3 {
            insert_event(&db, hour + chrono::Duration::minutes(5 + i * 10), "Chrome.exe");
        }

        let summarizer = Summarizer::new(db);
        let summary = summarizer.generate_hourly_summary(hour).unwrap();

        assert!(!summary.content.is_empty());
        assert!(summary.content.contains("Total events: 8"));
        assert!(summary.metrics["total_events"].as_u64().unwrap() == 8);
        assert!(summary.metrics["apps"].is_object());
    }

    #[test]
    fn test_daily_log_content() {
        let db = setup_db();
        let today = Utc::now().date_naive();
        let start = today.and_hms_opt(10, 0, 0).unwrap().and_utc();

        insert_event(&db, start, "Code.exe");
        insert_event(&db, start + chrono::Duration::hours(1), "Chrome.exe");

        let summarizer = Summarizer::new(db);
        let summary = summarizer.generate_daily_log(today).unwrap();

        assert!(!summary.content.is_empty());
        assert_eq!(summary.summary_type, SummaryType::Daily);
    }

    #[test]
    fn test_empty_period_summary() {
        let db = setup_db();
        let hour = Utc::now() - chrono::Duration::hours(100);
        let summarizer = Summarizer::new(db);
        let summary = summarizer.generate_hourly_summary(hour).unwrap();
        assert!(summary.content.contains("No activity"));
    }

    #[test]
    fn test_metrics_structure() {
        let db = setup_db();
        let hour = Utc::now() - chrono::Duration::hours(1);
        insert_event(&db, hour + chrono::Duration::minutes(5), "Code.exe");

        let summarizer = Summarizer::new(db);
        let summary = summarizer.generate_hourly_summary(hour).unwrap();

        let metrics = &summary.metrics;
        assert!(metrics.get("total_events").is_some());
        assert!(metrics.get("event_types").is_some());
        assert!(metrics.get("apps").is_some());
        assert!(metrics.get("projects").is_some());
    }
}

