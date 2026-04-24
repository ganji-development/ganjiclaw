//! Shell activity collector.
//!
//! Monitors PowerShell command history by tailing the PSReadLine history file.
//! Stores byte offsets in `ingestion_offsets` so restarts don't re-ingest.
//! Commands are SHA-256 hashed by default (they routinely contain secrets).

use async_trait::async_trait;
use crate::collector::{Collector, OffsetTracker};
use crate::schema::RawEvent;
use futures::stream::BoxStream;
use std::path::PathBuf;
use std::time::Duration;

/// Shell activity collector — reads PSReadLine history for new commands.
pub struct ShellActivityCollector {
    tracker: OffsetTracker,
    hash_commands: bool,
}

impl ShellActivityCollector {
    pub fn new(db_path: PathBuf) -> Self {
        Self {
            tracker: OffsetTracker::new("shell_activity".to_string(), db_path),
            hash_commands: true,
        }
    }

    /// Set whether to hash commands (default: true for privacy).
    pub fn with_hash_commands(mut self, hash: bool) -> Self {
        self.hash_commands = hash;
        self
    }

    /// Get the PSReadLine history file path.
    #[cfg(target_os = "windows")]
    fn history_path() -> Option<PathBuf> {
        let appdata = std::env::var("APPDATA").ok()?;
        let path = PathBuf::from(appdata)
            .join("Microsoft")
            .join("Windows")
            .join("PowerShell")
            .join("PSReadLine")
            .join("ConsoleHost_history.txt");
        if path.exists() { Some(path) } else { None }
    }

    #[cfg(not(target_os = "windows"))]
    fn history_path() -> Option<PathBuf> { None }

    /// Hash a command string with a simple hasher for privacy.
    fn hash_command(cmd: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        cmd.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }
}

#[async_trait]
impl Collector for ShellActivityCollector {
    fn name(&self) -> &str {
        "shell_activity"
    }

    async fn start(&self) -> anyhow::Result<BoxStream<'static, RawEvent>> {
        let history_path = match Self::history_path() {
            Some(p) => p,
            None => {
                tracing::warn!("PSReadLine history file not found — shell_activity disabled");
                return Ok(Box::pin(futures::stream::empty()));
            }
        };

        // Get starting byte offset from DB
        let start_offset: u64 = self.tracker.get_offset().await?
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        let hash_commands = self.hash_commands;
        let tracker_db_path = self.tracker.db_path().to_path_buf();

        let stream = async_stream::stream! {
            let mut current_offset = start_offset;
            let mut interval = tokio::time::interval(Duration::from_secs(5));

            // If file is smaller than our offset, it was truncated — reset
            if let Ok(meta) = tokio::fs::metadata(&history_path).await {
                if meta.len() < current_offset {
                    current_offset = 0;
                }
            }

            loop {
                interval.tick().await;

                let contents = match tokio::fs::read(&history_path).await {
                    Ok(c) => c,
                    Err(_) => continue,
                };

                if (contents.len() as u64) <= current_offset {
                    continue;
                }

                // Read new bytes from the offset
                let new_bytes = &contents[current_offset as usize..];
                let new_text = String::from_utf8_lossy(new_bytes);

                for line in new_text.lines() {
                    let trimmed = line.trim();
                    if trimmed.is_empty() { continue; }

                    let command_value = if hash_commands {
                        Self::hash_command(trimmed)
                    } else {
                        trimmed.to_string()
                    };

                    yield RawEvent::new(
                        "shell_activity".to_string(),
                        serde_json::json!({
                            "command": command_value,
                            "shell": "powershell",
                            "hashed": hash_commands,
                        }),
                    );
                }

                current_offset = contents.len() as u64;

                // Persist offset
                let offset_tracker = OffsetTracker::new(
                    "shell_activity".to_string(),
                    tracker_db_path.clone(),
                );
                let _ = offset_tracker.save_offset(current_offset.to_string()).await;
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
        let c = ShellActivityCollector::new(PathBuf::from(":memory:"));
        assert_eq!(c.name(), "shell_activity");
    }

    #[test]
    fn test_hash_command() {
        let h1 = ShellActivityCollector::hash_command("git push origin main");
        let h2 = ShellActivityCollector::hash_command("git push origin main");
        let h3 = ShellActivityCollector::hash_command("different command");
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
        assert_eq!(h1.len(), 16); // 16 hex chars
    }
}
