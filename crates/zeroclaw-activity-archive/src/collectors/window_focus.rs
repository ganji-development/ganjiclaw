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
    idle_threshold_seconds: u64,
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
    /// * `idle_threshold_seconds` - Seconds of inactivity before emitting user_idle (default: 120)
    pub fn new(db_path: std::path::PathBuf, poll_interval_seconds: u64, idle_threshold_seconds: u64) -> Self {
        Self {
            tracker: OffsetTracker::new("window_focus".to_string(), db_path),
            poll_interval: Duration::from_secs(poll_interval_seconds.max(1)),
            idle_threshold_seconds: idle_threshold_seconds.max(10),
            shutdown_tx: None,
        }
    }

    /// Get the current foreground window information.
    #[cfg(target_os = "windows")]
    fn get_foreground_window_info() -> Option<serde_json::Value> {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{
            OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
            QueryFullProcessImageNameW,
        };
        use windows::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
        };
        use windows::core::PWSTR;
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;
        use std::path::Path;

        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.is_invalid() {
                return None;
            }

            let mut title_buffer = [0u16; 512];
            let length = GetWindowTextW(hwnd, &mut title_buffer);
            let title = if length > 0 {
                OsString::from_wide(&title_buffer[..length as usize])
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };

            let mut process_id = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut process_id));

            let (process_name, process_path) = resolve_process_info(process_id)
                .unwrap_or_else(|| (format!("process_{}", process_id), String::new()));

            // Inner: OpenProcess → QueryFullProcessImageNameW → CloseHandle.
            // Returns (filename, full_path) on success; None on any failure
            // (system PIDs, access denied, PID-recycled since GetWindowThreadProcessId).
            unsafe fn resolve_process_info(pid: u32) -> Option<(String, String)> {
                let handle = unsafe {
                    OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?
                };
                if handle.is_invalid() {
                    return None;
                }

                let mut path_buffer = [0u16; 1024];
                let mut path_len: u32 = path_buffer.len() as u32;
                let result = unsafe {
                    QueryFullProcessImageNameW(
                        handle,
                        PROCESS_NAME_WIN32,
                        PWSTR(path_buffer.as_mut_ptr()),
                        &mut path_len,
                    )
                };
                let _ = unsafe { CloseHandle(handle) };

                result.ok()?;
                if path_len == 0 {
                    return None;
                }

                let process_path = OsString::from_wide(&path_buffer[..path_len as usize])
                    .to_string_lossy()
                    .into_owned();
                let process_name = Path::new(&process_path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&process_path)
                    .to_string();

                Some((process_name, process_path))
            }

            Some(serde_json::json!({
                "window_title": title,
                "process_id": process_id,
                "process_name": process_name,
                "process_path": process_path,
            }))
        }
    }

    #[cfg(not(target_os = "windows"))]
    fn get_foreground_window_info() -> Option<serde_json::Value> {
        // Stub implementation for non-Windows platforms
        None
    }

    /// Seconds since the last user input (keyboard or mouse) on the current
    /// window station. Returns None if the Win32 call fails (e.g. running as
    /// a service in session 0, where no interactive input exists).
    #[cfg(target_os = "windows")]
    fn get_idle_seconds() -> Option<u64> {
        use windows::Win32::System::SystemInformation::GetTickCount64;
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

        let mut info = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        unsafe { GetLastInputInfo(&mut info) }.ok().ok()?;

        let now_ms = unsafe { GetTickCount64() };
        let last_input_ms = u64::from(info.dwTime);
        if now_ms < last_input_ms {
            // Tick counter wrapped — report no idle time rather than underflow.
            return Some(0);
        }
        Some((now_ms - last_input_ms) / 1000)
    }

    #[cfg(not(target_os = "windows"))]
    fn get_idle_seconds() -> Option<u64> {
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
        let idle_threshold = self.idle_threshold_seconds;
        let (_shutdown_tx, mut shutdown_rx) = tokio::sync::broadcast::channel::<()>(1);

        let stream = async_stream::stream! {
            let mut last_window_info: Option<serde_json::Value> = None;
            let mut is_idle = false;
            let mut ticker = interval(poll_interval);
            ticker.tick().await; // Skip first tick

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        // Emit user_idle / user_active only on transitions.
                        // When idle, skip window-focus emission — whatever window
                        // happens to be foreground isn't being interacted with, so
                        // flooding the DB with duplicate titles is just noise.
                        match Self::get_idle_seconds() {
                            Some(secs) if secs >= idle_threshold && !is_idle => {
                                is_idle = true;
                                yield RawEvent::new(
                                    "user_idle".to_string(),
                                    serde_json::json!({ "idle_seconds": secs }),
                                );
                                continue;
                            }
                            Some(secs) if secs < idle_threshold && is_idle => {
                                is_idle = false;
                                yield RawEvent::new(
                                    "user_active".to_string(),
                                    serde_json::json!({ "idle_seconds_at_wake": secs }),
                                );
                            }
                            Some(_) if is_idle => continue, // still idle; skip focus poll
                            _ => {}
                        }

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
        let collector = WindowFocusCollector::new(db_path, 2, 120);
        assert_eq!(collector.name(), "window_focus");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_get_foreground_window_info() {
        let info = WindowFocusCollector::get_foreground_window_info();

        // Returns None on headless CI (no desktop). That's a valid path;
        // the contract is "don't panic." When a desktop is present, verify
        // the JSON shape is what the normalizer expects.
        let Some(info) = info else { return };

        let obj = info.as_object().expect("foreground info must be a JSON object");
        assert!(obj.contains_key("window_title"), "missing window_title");
        assert!(obj.contains_key("process_id"), "missing process_id");
        assert!(obj.contains_key("process_name"), "missing process_name");
        assert!(obj.contains_key("process_path"), "missing process_path");

        let pid = obj["process_id"].as_u64().expect("process_id must be number");
        assert!(pid > 0, "process_id must be a real PID");

        let name = obj["process_name"].as_str().expect("process_name must be string");
        assert!(!name.is_empty(), "process_name must be non-empty");

        // process_path is allowed to be empty (fallback when OpenProcess denies
        // access — typical for some system PIDs), but when present should look
        // like a real path.
        let path = obj["process_path"].as_str().expect("process_path must be string");
        if !path.is_empty() {
            assert!(
                path.contains('\\') || path.contains('/'),
                "process_path should be a path, got {path:?}"
            );
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_get_idle_seconds() {
        // Don't panic. On a live desktop the call succeeds; in session 0 it
        // may fail and return None — both are valid.
        let idle = WindowFocusCollector::get_idle_seconds();
        if let Some(secs) = idle {
            // Sanity: idle time should be far less than the uptime of this test run
            // (seconds, not years). If we get u64::MAX or something absurd, the
            // underflow guard in get_idle_seconds failed.
            assert!(secs < 60 * 60 * 24 * 365, "absurd idle time: {secs}s");
        }
    }
}
