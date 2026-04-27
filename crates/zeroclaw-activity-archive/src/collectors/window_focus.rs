//! Window focus collector for Windows.
//!
//! Tracks window activity by enumerating processes.
//! Uses Windows API - works in session 0 (services).

use async_trait::async_trait;
use crate::collector::{Collector, OffsetTracker};
use crate::schema::RawEvent;
use futures::stream::{BoxStream};
use std::time::Duration;
use tokio::time::interval;

/// Window focus collector.
///
/// Enumerates running processes and detects activity.
pub struct WindowFocusCollector {
    tracker: OffsetTracker,
    poll_interval: Duration,
    idle_threshold_seconds: u64,
    #[allow(dead_code)]
    shutdown_tx: Option<tokio::sync::broadcast::Sender<()>>,
}

impl WindowFocusCollector {
    pub fn new(db_path: std::path::PathBuf, poll_interval_seconds: u64, idle_threshold_seconds: u64) -> Self {
        Self {
            tracker: OffsetTracker::new("window_focus".to_string(), db_path),
            poll_interval: Duration::from_secs(poll_interval_seconds.max(1)),
            idle_threshold_seconds: idle_threshold_seconds.max(10),
            shutdown_tx: None,
        }
    }

    /// Get list of running processes (works in session 0).
    #[cfg(target_os = "windows")]
    fn get_active_windows() -> Vec<(String, u32)> {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        };
        use windows::Win32::System::ProcessStatus::EnumProcesses;
        use windows::core::PWSTR;
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;
        use std::path::Path;

        unsafe {
            let mut pids = vec![0u32; 4096];
            let mut bytes_returned = 0u32;
            
            if EnumProcesses(pids.as_mut_ptr(), (pids.len() * 4) as u32, &mut bytes_returned).is_err() {
                return Vec::new();
            }
            
            let count = bytes_returned as usize / 4;
            pids.truncate(count);
            
            let mut results: Vec<(String, u32)> = Vec::new();
            for pid in pids {
                if pid < 100 { continue; } // skip system PIDs
                
                let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid);
                if let Ok(h) = handle {
                    if !h.is_invalid() {
                        let mut buf = [0u16; 1024];
                        let mut len = buf.len() as u32;
                        if QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok() && len > 0 {
                            let path = OsString::from_wide(&buf[..len as usize]).to_string_lossy().into_owned();
                            let exe_name = Path::new(&path).file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or(&path)
                                .to_string();
                            results.push((exe_name, pid));
                            if results.len() >= 20 { break; } // limit
                        }
                        let _ = CloseHandle(h);
                    }
                }
            }
            results
        }
    }

    #[cfg(not(target_os = "windows"))]
    fn get_active_windows() -> Vec<(String, u32)> {
        Vec::new()
    }

    /// Get idle seconds - returns None in session 0.
    #[cfg(target_os = "windows")]
    fn get_idle_seconds() -> Option<u64> {
        use windows::Win32::System::SystemInformation::GetTickCount64;
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

        let mut info = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        if (unsafe { GetLastInputInfo(&mut info) } == windows::Win32::Foundation::FALSE) {
            return None;
        }

        let now_ms = unsafe { GetTickCount64() };
        let last_input_ms = u64::from(info.dwTime);
        if now_ms < last_input_ms {
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
            ticker.tick().await;

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        // Check idle state
                        match Self::get_idle_seconds() {
                            Some(secs) if secs >= idle_threshold && !is_idle => {
                                is_idle = true;
                                tracing::debug!("Idle after {}s", secs);
                                yield RawEvent::new("user_idle".to_string(), serde_json::json!({"idle_seconds": secs}));
                                continue;
                            }
                            Some(secs) if secs < idle_threshold && is_idle => {
                                is_idle = false;
                                tracing::debug!("Active (was idle {}s)", secs);
                                yield RawEvent::new("user_active".to_string(), serde_json::json!({"idle_seconds_at_wake": secs}));
                            }
                            Some(_) if is_idle => continue,
                            _ => {}
                        }

                        // Get process list
                        let processes = Self::get_active_windows();
                        for (name, pid) in processes {
                            let window_info = serde_json::json!({
                                "window_title": format!("{} - running", name),
                                "process_id": pid,
                                "process_name": name,
                            });
                            
                            if last_window_info.as_ref() != Some(&window_info) {
                                tracing::debug!("Process: {} (pid={})", name, pid);
                                let event = RawEvent::new("window_focus".to_string(), window_info.clone());
                                last_window_info = Some(window_info);
                                yield event;
                                break;
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
    fn test_collector_creation() {
        let db_path = std::path::PathBuf::from(":memory:");
        let collector = WindowFocusCollector::new(db_path, 2, 120);
        assert_eq!(collector.name(), "window_focus");
    }
}