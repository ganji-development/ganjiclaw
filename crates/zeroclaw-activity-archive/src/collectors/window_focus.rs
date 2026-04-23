//! Window focus collector for Windows.
//!
//! Tracks which window is currently active on the desktop.
//! Uses Windows API to poll for foreground window changes.

use async_trait::async_trait;
use crate::collector::{Collector, OffsetTracker};
use crate::schema::RawEvent;
use futures::stream::{BoxStream};
use std::time::Duration;
use tokio::time::interval;

/// Window focus collector.
///
/// Polls the Windows API every few seconds to detect
/// changes in the active window.
pub struct WindowFocusCollector {
    tracker: OffsetTracker,
    poll_interval: Duration,
    #[allow(dead_code)]
    shutdown_tx: Option<tokio::sync::broadcast::Sender<()>>,
}

impl WindowFocusCollector {
    /// Create a new window focus collector.
    ///
    /// # Arguments
    ///
    /// * `db_path` - Path to the activity archive database
    /// * `poll_interval_seconds` - How often to poll for window changes (default: 2)
    pub fn new(db_path: std::path::PathBuf, poll_interval_seconds: u64) -> Self {
        Self {
            tracker: OffsetTracker::new("window_focus".to_string(), db_path),
            poll_interval: Duration::from_secs(poll_interval_seconds.max(1)),
            shutdown_tx: None,
        }
    }

    /// Get the current foreground window information.
    #[cfg(target_os = "windows")]
    fn get_foreground_window_info() -> Option<serde_json::Value> {
        use windows::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
        };
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;

        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.is_invalid() {
                return None;
            }

            // Get window title
            let mut title_buffer = [0u16; 512];
            let length = GetWindowTextW(hwnd, &mut title_buffer);
            let title = if length > 0 {
                OsString::from_wide(&title_buffer[..length as usize])
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };

            // Get process ID
            let mut process_id = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut process_id));

            // Get process name (simplified - in production would use OpenProcess and QueryFullProcessImageName)
            let process_name = format!("process_{}", process_id);

            Some(serde_json::json!({
                "window_title": title,
                "process_id": process_id,
                "process_name": process_name,
            }))
        }
    }

    #[cfg(not(target_os = "windows"))]
    fn get_foreground_window_info() -> Option<serde_json::Value> {
        // Stub implementation for non-Windows platforms
        None
    }
}

#[async_trait]
impl Collector for WindowFocusCollector {
    fn name(&self) -> &str {
        "window_focus"
    }

    async fn start(&self) -> anyhow::Result<BoxStream<'static, RawEvent>> {
        let poll_interval = self.poll_interval;
        let (_shutdown_tx, mut shutdown_rx) = tokio::sync::broadcast::channel::<()>(1);

        let stream = async_stream::stream! {
            let mut last_window_info: Option<serde_json::Value> = None;
            let mut ticker = interval(poll_interval);
            ticker.tick().await; // Skip first tick

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        if let Some(window_info) = Self::get_foreground_window_info() {
                            // Only emit if window changed
                            if last_window_info.as_ref() != Some(&window_info) {
                                let event = RawEvent::new(
                                    "window_focus".to_string(),
                                    window_info.clone(),
                                );
                                last_window_info = Some(window_info);
                                yield event;
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        tracing::info!("Window focus collector shutting down");
                        break;
                    }
                }
            }
        };

        Ok(Box::pin(stream))
    }

    async fn get_offset(&self) -> anyhow::Result<Option<String>> {
        self.tracker.get_offset().await
    }

    async fn save_offset(&self, offset: String) -> anyhow::Result<()> {
        self.tracker.save_offset(offset).await
    }

    async fn stop(&self) -> anyhow::Result<()> {
        if let Some(tx) = &self.shutdown_tx {
            let _ = tx.send(());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_window_focus_collector_creation() {
        let db_path = std::path::PathBuf::from(":memory:");
        let collector = WindowFocusCollector::new(db_path, 2);
        assert_eq!(collector.name(), "window_focus");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_get_foreground_window_info() {
        let info = WindowFocusCollector::get_foreground_window_info();
        // On Windows with a desktop session, this should return Some
        // In headless environments, it might return None
        // We just verify it doesn't panic
    }
}
