//! Browser history collector.
//!
//! Reads browser history from Chrome, Edge, and Firefox SQLite databases.

use async_trait::async_trait;
use crate::collector::{Collector, OffsetTracker};
use crate::schema::RawEvent;
use futures::stream::BoxStream;

/// Browser history collector.
///
/// Polls browser history databases for new visits.
pub struct BrowserHistoryCollector {
    tracker: OffsetTracker,
}

impl BrowserHistoryCollector {
    /// Create a new browser history collector.
    pub fn new(db_path: std::path::PathBuf) -> Self {
        Self {
            tracker: OffsetTracker::new("browser_history".to_string(), db_path),
        }
    }
}

#[async_trait]
impl Collector for BrowserHistoryCollector {
    fn name(&self) -> &str {
        "browser_history"
    }

    async fn start(&self) -> anyhow::Result<BoxStream<'static, RawEvent>> {
        // TODO: Implement browser history polling
        // For now, return an empty stream
        let stream = futures::stream::empty();
        Ok(Box::pin(stream))
    }

    async fn get_offset(&self) -> anyhow::Result<Option<String>> {
        self.tracker.get_offset().await
    }

    async fn save_offset(&self, offset: String) -> anyhow::Result<()> {
        self.tracker.save_offset(offset).await
    }

    async fn stop(&self) -> anyhow::Result<()> {
        Ok(())
    }
}
