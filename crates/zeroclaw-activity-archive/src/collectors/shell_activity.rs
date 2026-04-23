//! Shell activity collector.
//!
//! Monitors PowerShell and CMD command history.

use async_trait::async_trait;
use crate::collector::{Collector, OffsetTracker};
use crate::schema::RawEvent;
use futures::stream::BoxStream;

/// Shell activity collector.
///
/// Reads shell history files for new commands.
pub struct ShellActivityCollector {
    tracker: OffsetTracker,
}

impl ShellActivityCollector {
    /// Create a new shell activity collector.
    pub fn new(db_path: std::path::PathBuf) -> Self {
        Self {
            tracker: OffsetTracker::new("shell_activity".to_string(), db_path),
        }
    }
}

#[async_trait]
impl Collector for ShellActivityCollector {
    fn name(&self) -> &str {
        "shell_activity"
    }

    async fn start(&self) -> anyhow::Result<BoxStream<'static, RawEvent>> {
        // TODO: Implement shell history monitoring
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
