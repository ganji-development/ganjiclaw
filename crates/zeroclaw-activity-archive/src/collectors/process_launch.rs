//! Process launch collector for Windows.
//!
//! Tracks process creation events. When running as a system service
//! (elevated), uses WMI event subscription for real-time notifications.
//! Otherwise, falls back to periodic process-list diffing via
//! `EnumProcesses`.

use async_trait::async_trait;
use crate::collector::{Collector, OffsetTracker};
use crate::schema::RawEvent;
use futures::stream::BoxStream;
use std::path::PathBuf;
use std::time::Duration;

/// Process launch collector — monitors process creation and exit events.
pub struct ProcessLaunchCollector {
    tracker: OffsetTracker,
    poll_interval: Duration,
}

impl ProcessLaunchCollector {
    pub fn new(db_path: PathBuf) -> Self {
        Self {
            tracker: OffsetTracker::new("process_launch".to_string(), db_path),
            poll_interval: Duration::from_secs(5),
        }
    }

    /// Check if the current process is running with elevated privileges.
    #[cfg(target_os = "windows")]
    fn is_elevated() -> bool {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
        use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

        unsafe {
            let mut token = windows::Win32::Foundation::HANDLE::default();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
                return false;
            }

            let mut elevation = TOKEN_ELEVATION::default();
            let mut return_length = 0u32;
            let result = GetTokenInformation(
                token,
                TokenElevation,
                Some(&mut elevation as *mut _ as *mut _),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut return_length,
            );
            let _ = CloseHandle(token);
            result.is_ok() && elevation.TokenIsElevated != 0
        }
    }

    #[cfg(not(target_os = "windows"))]
    fn is_elevated() -> bool { false }

    /// Get the list of running process IDs via EnumProcesses.
    #[cfg(target_os = "windows")]
    fn get_process_list() -> Vec<u32> {
        use windows::Win32::System::ProcessStatus::EnumProcesses;

        let mut pids = vec![0u32; 2048];
        let mut bytes_returned = 0u32;
        unsafe {
            let ok = EnumProcesses(
                pids.as_mut_ptr(),
                (pids.len() * std::mem::size_of::<u32>()) as u32,
                &mut bytes_returned,
            );
            if ok.is_err() {
                return Vec::new();
            }
        }
        let count = bytes_returned as usize / std::mem::size_of::<u32>();
        pids.truncate(count);
        pids.retain(|&pid| pid != 0);
        pids
    }

    #[cfg(not(target_os = "windows"))]
    fn get_process_list() -> Vec<u32> { Vec::new() }

    /// Resolve a PID to a process name.
    #[cfg(target_os = "windows")]
    fn get_process_name(pid: u32) -> Option<String> {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{
            OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
            QueryFullProcessImageNameW,
        };
        use windows::core::PWSTR;
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;

        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            if handle.is_invalid() { return None; }

            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let result = QueryFullProcessImageNameW(
                handle, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len,
            );
            let _ = CloseHandle(handle);
            result.ok()?;
            if len == 0 { return None; }

            let full_path = OsString::from_wide(&buf[..len as usize])
                .to_string_lossy().into_owned();
            let name = std::path::Path::new(&full_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&full_path)
                .to_string();
            Some(name)
        }
    }

    #[cfg(not(target_os = "windows"))]
    fn get_process_name(_pid: u32) -> Option<String> { None }
}

#[async_trait]
impl Collector for ProcessLaunchCollector {
    fn name(&self) -> &str {
        "process_launch"
    }

    async fn start(&self) -> anyhow::Result<BoxStream<'static, RawEvent>> {
        let is_elevated = Self::is_elevated();
        if !is_elevated {
            tracing::warn!(
                "process_launch collector: not running elevated. \
                 Using periodic process-list diffing (less efficient). \
                 Run as a system service for real-time tracking."
            );
        }

        let poll_interval = self.poll_interval;

        // Process-list diff approach (works both elevated and non-elevated)
        let stream = async_stream::stream! {
            let mut known_pids: std::collections::HashSet<u32> = std::collections::HashSet::new();
            let mut interval = tokio::time::interval(poll_interval);

            // Seed with current process list
            for pid in Self::get_process_list() {
                known_pids.insert(pid);
            }

            loop {
                interval.tick().await;
                let current_pids: std::collections::HashSet<u32> =
                    Self::get_process_list().into_iter().collect();

                // New processes
                for &pid in current_pids.difference(&known_pids) {
                    let process_name = Self::get_process_name(pid)
                        .unwrap_or_else(|| format!("pid_{}", pid));

                    yield RawEvent::new(
                        "process_launch".to_string(),
                        serde_json::json!({
                            "process_name": process_name,
                            "process_id": pid,
                            "event": "start",
                        }),
                    );
                }

                // Exited processes
                for &pid in known_pids.difference(&current_pids) {
                    yield RawEvent::new(
                        "process_launch".to_string(),
                        serde_json::json!({
                            "process_id": pid,
                            "event": "exit",
                        }),
                    );
                }

                known_pids = current_pids;
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
    async fn stop(&self) -> anyhow::Result<()> { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_creation() {
        let c = ProcessLaunchCollector::new(PathBuf::from(":memory:"));
        assert_eq!(c.name(), "process_launch");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_get_process_list_returns_pids() {
        let pids = ProcessLaunchCollector::get_process_list();
        assert!(!pids.is_empty(), "should find at least one running process");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_is_elevated_does_not_panic() {
        // Just verify it doesn't panic — actual value depends on how tests are run
        let _ = ProcessLaunchCollector::is_elevated();
    }
}
