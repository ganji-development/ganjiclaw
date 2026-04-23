//! Process launch collector for Windows.
//!
//! Tracks process creation and termination events.
//! Uses Windows Event Log or WMI to detect process activity.

use async_trait::async_trait;
use crate::collector::{Collector, OffsetTracker};
use crate::schema::RawEvent;
use futures::stream::BoxStream;

/// Process launch collector.
///
/// Monitors process creation and exit events.
pub struct ProcessLaunchCollector {
    tracker: OffsetTracker,
}

impl ProcessLaunchCollector {
    /// Create a new process launch collector.
    pub fn new(db_path: std::path::PathBuf) -> Self {
        Self {
            tracker: OffsetTracker::new("process_launch".to_string(), db_path),
        }
    }
}

#[async_trait]
impl Collector for ProcessLaunchCollector {
    fn name(&self) -> &str {
        "process_launch"
    }

    async fn start(&self) -> anyhow::Result<BoxStream<'static, RawEvent>> {
        // TODO: Implement Windows Event Log subscription for process events
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
