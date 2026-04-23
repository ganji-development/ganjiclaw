//! File activity collector.
//!
//! Monitors file system changes in selected folders.

use async_trait::async_trait;
use crate::collector::{Collector, OffsetTracker};
use crate::schema::RawEvent;
use futures::stream::BoxStream;

/// File activity collector.
///
/// Watches configured folders for file changes.
#[allow(dead_code)]
pub struct FileActivityCollector {
    tracker: OffsetTracker,
    folders: Vec<std::path::PathBuf>,
}

impl FileActivityCollector {
    /// Create a new file activity collector.
    ///
    /// # Arguments
    ///
    /// * `db_path` - Path to the activity archive database
    /// * `folders` - List of folders to monitor
    pub fn new(db_path: std::path::PathBuf, folders: Vec<std::path::PathBuf>) -> Self {
        Self {
            tracker: OffsetTracker::new("file_activity".to_string(), db_path),
            folders,
        }
    }
}

#[async_trait]
impl Collector for FileActivityCollector {
    fn name(&self) -> &str {
        "file_activity"
    }

    async fn start(&self) -> anyhow::Result<BoxStream<'static, RawEvent>> {
        // TODO: Implement file system watching using notify crate
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
