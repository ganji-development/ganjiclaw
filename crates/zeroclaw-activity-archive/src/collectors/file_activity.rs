//! File activity collector.
//!
//! Monitors file system changes in selected folders using the `notify` crate.

use async_trait::async_trait;
use crate::collector::{Collector, OffsetTracker};
use crate::schema::RawEvent;
use futures::stream::BoxStream;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const DEBOUNCE_MS: u64 = 500;

/// File activity collector — watches configured folders for file changes.
pub struct FileActivityCollector {
    tracker: OffsetTracker,
    folders: Vec<PathBuf>,
    exclude_paths: Vec<String>,
}

impl FileActivityCollector {
    pub fn new(db_path: PathBuf, folders: Vec<PathBuf>) -> Self {
        Self {
            tracker: OffsetTracker::new("file_activity".to_string(), db_path),
            folders,
            exclude_paths: Vec::new(),
        }
    }

    pub fn with_exclude_paths(mut self, patterns: Vec<String>) -> Self {
        self.exclude_paths = patterns;
        self
    }

    fn is_excluded(path: &std::path::Path, patterns: &[String]) -> bool {
        let path_str = path.to_string_lossy();
        for pattern in patterns {
            if pattern.contains('*') {
                let regex_pattern = pattern
                    .replace('.', r"\.")
                    .replace("**", "§§")
                    .replace('*', "[^/\\\\]*")
                    .replace("§§", ".*")
                    .replace('?', ".");
                if let Ok(re) = regex::Regex::new(&format!("(?i){}", regex_pattern)) {
                    if re.is_match(&path_str) {
                        return true;
                    }
                }
            } else if path_str.contains(pattern) {
                return true;
            }
        }
        false
    }

    #[cfg(target_os = "windows")]
    fn event_kind_to_action(kind: &notify::EventKind) -> Option<&'static str> {
        use notify::EventKind;
        match kind {
            EventKind::Create(_) => Some("create"),
            EventKind::Modify(_) => Some("modify"),
            EventKind::Remove(_) => Some("delete"),
            _ => None,
        }
    }
}

#[async_trait]
impl Collector for FileActivityCollector {
    fn name(&self) -> &str {
        "file_activity"
    }

    #[cfg(target_os = "windows")]
    async fn start(&self) -> anyhow::Result<BoxStream<'static, RawEvent>> {
        use notify::{Watcher, RecursiveMode};
        use tokio::sync::mpsc;

        let (tx, mut rx) = mpsc::channel::<notify::Event>(256);
        let folders = self.folders.clone();
        let exclude_paths = self.exclude_paths.clone();

        std::thread::spawn(move || {
            let rt_tx = tx;
            let mut watcher = match notify::recommended_watcher(
                move |res: Result<notify::Event, notify::Error>| {
                    if let Ok(event) = res {
                        let _ = rt_tx.blocking_send(event);
                    }
                },
            ) {
                Ok(w) => w,
                Err(e) => {
                    tracing::error!("Failed to create file watcher: {}", e);
                    return;
                }
            };

            for folder in &folders {
                if folder.exists() {
                    if let Err(e) = watcher.watch(folder, RecursiveMode::Recursive) {
                        tracing::warn!("Failed to watch {:?}: {}", folder, e);
                    }
                }
            }
            loop { std::thread::sleep(Duration::from_secs(3600)); }
        });

        let stream = async_stream::stream! {
            let mut debounce_map: HashMap<(String, String), Instant> = HashMap::new();
            let debounce_window = Duration::from_millis(DEBOUNCE_MS);

            while let Some(event) = rx.recv().await {
                let action = match Self::event_kind_to_action(&event.kind) {
                    Some(a) => a,
                    None => continue,
                };
                for path in &event.paths {
                    if Self::is_excluded(path, &exclude_paths) { continue; }
                    let path_str = path.to_string_lossy().to_string();
                    let key = (path_str.clone(), action.to_string());
                    let now = Instant::now();
                    if let Some(last) = debounce_map.get(&key) {
                        if now.duration_since(*last) < debounce_window { continue; }
                    }
                    debounce_map.insert(key, now);
                    yield RawEvent::new(
                        "file_activity".to_string(),
                        serde_json::json!({ "action": action, "path": path_str }),
                    );
                }
                if debounce_map.len() > 1000 {
                    let cutoff = Instant::now() - Duration::from_secs(10);
                    debounce_map.retain(|_, v| *v > cutoff);
                }
            }
        };
        Ok(Box::pin(stream))
    }

    #[cfg(not(target_os = "windows"))]
    async fn start(&self) -> anyhow::Result<BoxStream<'static, RawEvent>> {
        Ok(Box::pin(futures::stream::empty()))
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
        let c = FileActivityCollector::new(PathBuf::from(":memory:"), vec![]);
        assert_eq!(c.name(), "file_activity");
    }

    #[test]
    fn test_is_excluded() {
        let pats = vec!["**/.ssh/**".into(), "**/node_modules/**".into()];
        assert!(FileActivityCollector::is_excluded(
            std::path::Path::new("C:\\Users\\.ssh\\id_rsa"), &pats));
        assert!(!FileActivityCollector::is_excluded(
            std::path::Path::new("C:\\project\\src\\main.rs"), &pats));
    }
}
