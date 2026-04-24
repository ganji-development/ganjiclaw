//! Session inference and management.
//!
//! Groups events into sessions based on time proximity and context.

use crate::schema::{Event, Session, SessionLabel};
use parking_lot::Mutex;
use rusqlite::{Connection, params};
use std::sync::Arc;
use anyhow::Result;
use chrono::{DateTime, Duration, Utc};

/// Sessionizer for grouping events into sessions.
pub struct Sessionizer {
    db: Arc<Mutex<Connection>>,
    idle_timeout_minutes: u64,
    context_switch_threshold_minutes: u64,
}

impl Sessionizer {
    /// Create a new sessionizer.
    ///
    /// # Arguments
    ///
    /// * `db` - Database connection
    /// * `idle_timeout_minutes` - Minutes of inactivity before ending a session
    /// * `context_switch_threshold_minutes` - Minutes before a context switch creates a new session
    pub fn new(db: Arc<Mutex<Connection>>, idle_timeout_minutes: u64, context_switch_threshold_minutes: u64) -> Self {
        Self {
            db,
            idle_timeout_minutes,
            context_switch_threshold_minutes,
        }
    }

    /// Update sessions based on unassigned events.
    pub fn update_sessions(&self) -> Result<()> {
        // Find events not yet assigned to a session
        let unassigned_events = self.get_unassigned_events()?;

        if unassigned_events.is_empty() {
            return Ok(());
        }

        // Group events into sessions
        let sessions = self.group_events_into_sessions(&unassigned_events)?;

        // Store sessions and link events
        for session in sessions {
            self.store_session(&session)?;
            self.link_events_to_session(&session)?;
        }

        Ok(())
    }

    /// Get events not yet assigned to a session.
    fn get_unassigned_events(&self) -> Result<Vec<Event>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare(
            "SELECT id, ts_utc, ts_local, source, event_type, actor, host, app, title, path,
                    details_json, sensitivity, project_key, session_id, hash, raw_ref, created_at
             FROM events
             WHERE session_id IS NULL
             ORDER BY ts_utc ASC"
        )?;

        let events = stmt.query_map([], |row| {
            Ok(Event {
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

    /// Group events into sessions.
    fn group_events_into_sessions(&self, events: &[Event]) -> Result<Vec<Session>> {
        if events.is_empty() {
            return Ok(Vec::new());
        }

        let mut sessions = Vec::new();
        let mut current_session = Session::new(events[0].ts_utc);
        let mut last_event_time = events[0].ts_utc;
        let mut last_context = self.extract_context(&events[0]);

        for event in events.iter() {
            let time_gap = event.ts_utc.signed_duration_since(last_event_time);
            let idle_threshold = Duration::minutes(self.idle_timeout_minutes as i64);
            let context_threshold = Duration::minutes(self.context_switch_threshold_minutes as i64);

            let current_context = self.extract_context(event);

            // Check if we should start a new session
            let should_start_new_session = time_gap > idle_threshold
                || (time_gap > context_threshold && current_context != last_context);

            if should_start_new_session {
                // Finalize current session
                current_session.end_ts_utc = Some(last_event_time);
                current_session.label = self.infer_session_label(&current_session);
                sessions.push(current_session.clone());

                // Start new session
                current_session = Session::new(event.ts_utc);
            }

            // Update session
            current_session.event_count += 1;
            if let Some(project_key) = &event.project_key {
                current_session.project_key = Some(project_key.clone());
            }

            last_event_time = event.ts_utc;
            last_context = current_context;
        }

        // Don't forget the last session
        current_session.end_ts_utc = Some(last_event_time);
        current_session.label = self.infer_session_label(&current_session);
        sessions.push(current_session);

        Ok(sessions)
    }

    /// Extract context from an event.
    fn extract_context(&self, event: &Event) -> String {
        // Context is defined by app + project
        let app = event.app.as_deref().unwrap_or("unknown");
        let project = event.project_key.as_deref().unwrap_or("none");
        format!("{}:{}", app, project)
    }

    /// Infer session label from events.
    fn infer_session_label(&self, session: &Session) -> SessionLabel {
        // This is a simplified implementation
        // In production, would analyze event patterns more thoroughly

        if let Some(_project_key) = &session.project_key {
            // If we have a project, it's likely coding or research
            return SessionLabel::Coding;
        }

        // Default to unknown
        SessionLabel::Unknown
    }

    /// Store a session in the database.
    fn store_session(&self, session: &Session) -> Result<()> {
        let conn = self.db.lock();

        conn.execute(
            "INSERT INTO sessions (
                id, start_ts_utc, end_ts_utc, label, project_key, tags, summary, event_count, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                session.id,
                session.start_ts_utc.to_rfc3339(),
                session.end_ts_utc.map(|t| t.to_rfc3339()),
                session.label.as_str(),
                session.project_key,
                session.tags.join(","),
                session.summary,
                session.event_count,
                session.created_at.to_rfc3339(),
            ],
        )?;

        Ok(())
    }

    /// Link events to a session.
    fn link_events_to_session(&self, session: &Session) -> Result<()> {
        let conn = self.db.lock();

        // Update events that fall within this session's time range
        conn.execute(
            "UPDATE events
             SET session_id = ?1
             WHERE ts_utc >= ?2
             AND ts_utc <= ?3
             AND session_id IS NULL",
            params![
                session.id,
                session.start_ts_utc.to_rfc3339(),
                session.end_ts_utc.map(|t| t.to_rfc3339()).unwrap_or_else(|| Utc::now().to_rfc3339()),
            ],
        )?;

        Ok(())
    }

    /// Get active session (if any).
    pub fn get_active_session(&self) -> Result<Option<Session>> {
        let conn = self.db.lock();

        let mut stmt = conn.prepare(
            "SELECT id, start_ts_utc, end_ts_utc, label, project_key, tags, summary, event_count, created_at
             FROM sessions
             WHERE end_ts_utc IS NULL
             ORDER BY start_ts_utc DESC
             LIMIT 1"
        )?;

        let result = stmt.query_row([], |row| {
            Ok(Session {
                id: row.get(0)?,
                start_ts_utc: DateTime::parse_from_rfc3339(&row.get::<_, String>(1)?)
                    .unwrap()
                    .with_timezone(&Utc),
                end_ts_utc: row.get::<_, Option<String>>(2)?
                    .map(|s| DateTime::parse_from_rfc3339(&s).unwrap().with_timezone(&Utc)),
                label: SessionLabel::from_str(&row.get::<_, String>(3)?)
                    .unwrap_or(SessionLabel::Unknown),
                project_key: row.get(4)?,
                tags: row.get::<_, String>(5)?
                    .split(',')
                    .map(|s| s.to_string())
                    .collect(),
                summary: row.get(6)?,
                event_count: row.get(7)?,
                created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(8)?)
                    .unwrap()
                    .with_timezone(&Utc),
            })
        });

        match result {
            Ok(session) => Ok(Some(session)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// End the active session.
    pub fn end_active_session(&self) -> Result<()> {
        let conn = self.db.lock();

        conn.execute(
            "UPDATE sessions
             SET end_ts_utc = ?1
             WHERE end_ts_utc IS NULL",
            params![Utc::now().to_rfc3339()],
        )?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{init_schema, Event, EventType};
    use parking_lot::Mutex;
    use std::sync::Arc;

    fn setup_db() -> Arc<Mutex<Connection>> {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        Arc::new(Mutex::new(conn))
    }

    fn insert_event(db: &Arc<Mutex<Connection>>, ts: DateTime<Utc>, app: &str, project: Option<&str>) {
        let event = Event {
            id: uuid::Uuid::new_v4().to_string(),
            ts_utc: ts,
            ts_local: chrono::Local::now(),
            source: "window_focus".to_string(),
            event_type: EventType::WindowFocus,
            actor: None,
            host: None,
            app: Some(app.to_string()),
            title: Some(format!("{} window", app)),
            path: None,
            details: serde_json::json!({}),
            sensitivity: 0,
            project_key: project.map(|s| s.to_string()),
            session_id: None,
            hash: None,
            raw_ref: None,
            created_at: Utc::now(),
        };
        let conn = db.lock();
        conn.execute(
            "INSERT INTO events (id, ts_utc, ts_local, source, event_type, app, title, details_json, sensitivity, project_key, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                event.id, event.ts_utc.to_rfc3339(), event.ts_local.to_rfc3339(),
                event.source, event.event_type.as_str(), event.app, event.title,
                serde_json::to_string(&event.details).unwrap(), event.sensitivity,
                event.project_key, event.created_at.to_rfc3339(),
            ],
        ).unwrap();
    }

    #[test]
    fn test_single_session_continuous_activity() {
        let db = setup_db();
        let base = Utc::now();

        // 5 events, 1 minute apart — should form one session
        for i in 0..5 {
            insert_event(&db, base + Duration::minutes(i), "Code.exe", Some("proj"));
        }

        let sessionizer = Sessionizer::new(db.clone(), 30, 15);
        sessionizer.update_sessions().unwrap();

        let conn = db.lock();
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 1, "continuous activity should produce one session");
    }

    #[test]
    fn test_idle_timeout_creates_new_session() {
        let db = setup_db();
        let base = Utc::now();

        // First burst of activity
        for i in 0..3 {
            insert_event(&db, base + Duration::minutes(i), "Code.exe", Some("proj"));
        }
        // Gap of 45 minutes (> 30 min idle threshold)
        for i in 0..3 {
            insert_event(&db, base + Duration::minutes(48 + i), "Code.exe", Some("proj"));
        }

        let sessionizer = Sessionizer::new(db.clone(), 30, 15);
        sessionizer.update_sessions().unwrap();

        let conn = db.lock();
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 2, "idle gap should split into two sessions");
    }

    #[test]
    fn test_context_switch_creates_new_session() {
        let db = setup_db();
        let base = Utc::now();

        // Working in Code.exe on project A
        for i in 0..3 {
            insert_event(&db, base + Duration::minutes(i), "Code.exe", Some("projA"));
        }
        // Switch to Chrome on different project after 20 min gap (> 15 min context switch)
        for i in 0..3 {
            insert_event(&db, base + Duration::minutes(23 + i), "Chrome.exe", Some("projB"));
        }

        let sessionizer = Sessionizer::new(db.clone(), 30, 15);
        sessionizer.update_sessions().unwrap();

        let conn = db.lock();
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 2, "context switch should create a new session");
    }
}

