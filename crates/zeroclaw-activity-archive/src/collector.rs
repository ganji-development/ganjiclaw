//! Collector trait and framework for activity data collection.

use async_trait::async_trait;
use crate::schema::RawEvent;
use futures::stream::BoxStream;
use rusqlite::params;

/// Trait for activity data collectors.
///
/// Collectors observe system activity and emit raw events.
/// Each collector is responsible for its own data source and
/// must handle offset tracking for resumability.
#[async_trait]
pub trait Collector: Send + Sync {
    /// Get the collector name.
    fn name(&self) -> &str;

    /// Start collecting events.
    ///
    /// Returns a stream of raw events. The stream should continue
    /// emitting events until the collector is stopped or an error occurs.
    async fn start(&self) -> anyhow::Result<BoxStream<'static, RawEvent>>;

    /// Get the last processed offset for this collector.
    ///
    /// Offsets are collector-specific and can be timestamps,
    /// cursors, or any other identifier that allows resuming
    /// from where we left off.
    async fn get_offset(&self) -> anyhow::Result<Option<String>>;

    /// Save the current offset checkpoint.
    ///
    /// This should be called periodically to ensure progress
    /// is not lost if the collector crashes.
    async fn save_offset(&self, offset: String) -> anyhow::Result<()>;

    /// Stop collecting events.
    ///
    /// This should gracefully shut down the collector and
    /// ensure any buffered events are flushed.
    async fn stop(&self) -> anyhow::Result<()>;
}

/// Base implementation for collectors that need offset tracking.
pub struct OffsetTracker {
    collector_name: String,
    db_path: std::path::PathBuf,
}

impl OffsetTracker {
    /// Create a new offset tracker for a collector.
    pub fn new(collector_name: String, db_path: std::path::PathBuf) -> Self {
        Self {
            collector_name,
            db_path,
        }
    }

    /// Get a reference to the database path.
    pub fn db_path(&self) -> &std::path::Path {
        &self.db_path
    }

    /// Get the last offset from the database.
    pub async fn get_offset(&self) -> anyhow::Result<Option<String>> {
        use rusqlite::Connection;

        let conn = Connection::open(&self.db_path)?;
        let mut stmt = conn.prepare(
            "SELECT offset_value FROM ingestion_offsets WHERE collector_name = ?1"
        )?;

        let result = stmt.query_row(params![&self.collector_name], |row| {
            row.get(0)
        });

        match result {
            Ok(offset) => Ok(Some(offset)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Save the current offset to the database.
    pub async fn save_offset(&self, offset: String) -> anyhow::Result<()> {
        use rusqlite::params;
        use chrono::Utc;

        let conn = rusqlite::Connection::open(&self.db_path)?;

        conn.execute(
            "INSERT OR REPLACE INTO ingestion_offsets (collector_name, offset_value, updated_at)
             VALUES (?1, ?2, ?3)",
            params![&self.collector_name, &offset, Utc::now().to_rfc3339()],
        )?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_offset_tracker() {
        // Test offset tracker functionality
        let db_path = std::path::PathBuf::from(":memory:");
        let tracker = OffsetTracker::new("test_collector".to_string(), db_path);

        // Initialize schema
        let conn = rusqlite::Connection::open(":memory:").unwrap();
        crate::schema::init_schema(&conn).unwrap();

        // Test get_offset (should return None initially)
        // Note: This is a basic test structure
        // Full integration tests would be in a separate test module
    }
}
