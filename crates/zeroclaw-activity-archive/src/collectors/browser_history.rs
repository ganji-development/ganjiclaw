//! Browser history collector.
//!
//! Reads browser history from Chrome, Edge, and Firefox SQLite databases.
//! Copies each DB to a temp file before reading (live files are locked by
//! the browser process). Tracks `last_visit_time` per browser in
//! `ingestion_offsets` to avoid re-ingestion on restart.

use async_trait::async_trait;
use crate::collector::{Collector, OffsetTracker};
use crate::schema::RawEvent;
use futures::stream::BoxStream;
use std::path::PathBuf;
use std::time::Duration;

/// Browser history collector — polls browser history DBs for new visits.
pub struct BrowserHistoryCollector {
    tracker: OffsetTracker,
    db_path: PathBuf,
    exclude_domains: Vec<String>,
    poll_interval: Duration,
}

impl BrowserHistoryCollector {
    pub fn new(db_path: PathBuf) -> Self {
        Self {
            tracker: OffsetTracker::new("browser_history".to_string(), db_path.clone()),
            db_path,
            exclude_domains: Vec::new(),
            poll_interval: Duration::from_secs(60),
        }
    }

    pub fn with_exclude_domains(mut self, domains: Vec<String>) -> Self {
        self.exclude_domains = domains;
        self
    }

    pub fn with_poll_interval(mut self, secs: u64) -> Self {
        self.poll_interval = Duration::from_secs(secs.max(10));
        self
    }

    /// Known browser history DB locations on Windows.
    #[cfg(target_os = "windows")]
    fn browser_sources() -> Vec<(&'static str, PathBuf, &'static str)> {
        let mut sources = Vec::new();
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            // Chrome
            let chrome = PathBuf::from(&local)
                .join("Google").join("Chrome").join("User Data").join("Default").join("History");
            if chrome.exists() {
                sources.push(("chrome", chrome, "chromium"));
            }
            // Edge
            let edge = PathBuf::from(&local)
                .join("Microsoft").join("Edge").join("User Data").join("Default").join("History");
            if edge.exists() {
                sources.push(("edge", edge, "chromium"));
            }
        }
        if let Ok(appdata) = std::env::var("APPDATA") {
            // Firefox — find profile directory
            let profiles_dir = PathBuf::from(&appdata)
                .join("Mozilla").join("Firefox").join("Profiles");
            if profiles_dir.exists() {
                if let Ok(entries) = std::fs::read_dir(&profiles_dir) {
                    for entry in entries.flatten() {
                        let places = entry.path().join("places.sqlite");
                        if places.exists() {
                            sources.push(("firefox", places, "firefox"));
                            break; // Use first profile found
                        }
                    }
                }
            }
        }
        sources
    }

    #[cfg(not(target_os = "windows"))]
    fn browser_sources() -> Vec<(&'static str, PathBuf, &'static str)> {
        Vec::new()
    }

    /// Copy a browser DB to a temp file and query new visits.
    fn query_chromium_history(
        source_path: &std::path::Path,
        last_visit_time: i64,
    ) -> anyhow::Result<Vec<(String, String, i64)>> {
        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.join(format!("zc_browser_{}.db", uuid::Uuid::new_v4()));
        std::fs::copy(source_path, &temp_path)?;

        let conn = rusqlite::Connection::open_with_flags(
            &temp_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;

        let mut stmt = conn.prepare(
            "SELECT u.url, u.title, v.visit_time
             FROM visits v JOIN urls u ON v.url = u.id
             WHERE v.visit_time > ?1
             ORDER BY v.visit_time ASC
             LIMIT 200"
        )?;

        let rows: Vec<(String, String, i64)> = stmt
            .query_map(rusqlite::params![last_visit_time], |row| {
                Ok((row.get(0)?, row.get::<_, String>(1).unwrap_or_default(), row.get(2)?))
            })?
            .filter_map(|r| r.ok())
            .collect();

        let _ = std::fs::remove_file(&temp_path);
        Ok(rows)
    }

    fn query_firefox_history(
        source_path: &std::path::Path,
        last_visit_date: i64,
    ) -> anyhow::Result<Vec<(String, String, i64)>> {
        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.join(format!("zc_browser_{}.db", uuid::Uuid::new_v4()));
        std::fs::copy(source_path, &temp_path)?;

        let conn = rusqlite::Connection::open_with_flags(
            &temp_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;

        let mut stmt = conn.prepare(
            "SELECT p.url, p.title, h.visit_date
             FROM moz_historyvisits h JOIN moz_places p ON h.place_id = p.id
             WHERE h.visit_date > ?1
             ORDER BY h.visit_date ASC
             LIMIT 200"
        )?;

        let rows: Vec<(String, String, i64)> = stmt
            .query_map(rusqlite::params![last_visit_date], |row| {
                Ok((row.get(0)?, row.get::<_, String>(1).unwrap_or_default(), row.get(2)?))
            })?
            .filter_map(|r| r.ok())
            .collect();

        let _ = std::fs::remove_file(&temp_path);
        Ok(rows)
    }

    fn is_domain_excluded(url: &str, exclude_domains: &[String]) -> bool {
        if let Ok(parsed) = url::Url::parse(url) {
            if let Some(host) = parsed.host_str() {
                let host_lower = host.to_lowercase();
                for pattern in exclude_domains {
                    let pat = pattern.to_lowercase();
                    if pat.starts_with("*.") {
                        let suffix = &pat[1..]; // ".example.com"
                        if host_lower.ends_with(suffix) || host_lower == pat[2..] {
                            return true;
                        }
                    } else if host_lower == pat {
                        return true;
                    }
                }
            }
        }
        false
    }
}

#[async_trait]
impl Collector for BrowserHistoryCollector {
    fn name(&self) -> &str {
        "browser_history"
    }

    async fn start(&self) -> anyhow::Result<BoxStream<'static, RawEvent>> {
        let sources = Self::browser_sources();
        if sources.is_empty() {
            tracing::warn!("No browser history databases found");
            return Ok(Box::pin(futures::stream::empty()));
        }

        let poll_interval = self.poll_interval;
        let exclude_domains = self.exclude_domains.clone();
        let tracker_db_path = self.db_path.clone();

        let stream = async_stream::stream! {
            let mut interval = tokio::time::interval(poll_interval);
            // Per-browser last visit time offsets
            let mut offsets: std::collections::HashMap<String, i64> = std::collections::HashMap::new();

            // Load saved offsets
            for (name, _, _) in &sources {
                let tracker = OffsetTracker::new(
                    format!("browser_history_{}", name),
                    tracker_db_path.clone(),
                );
                if let Ok(Some(offset_str)) = tracker.get_offset().await {
                    if let Ok(v) = offset_str.parse::<i64>() {
                        offsets.insert(name.to_string(), v);
                    }
                }
            }

            loop {
                interval.tick().await;

                for (name, path, db_type) in &sources {
                    let last_time = offsets.get(*name).copied().unwrap_or(0);

                    let results = match *db_type {
                        "chromium" => Self::query_chromium_history(path, last_time),
                        "firefox" => Self::query_firefox_history(path, last_time),
                        _ => continue,
                    };

                    match results {
                        Ok(rows) => {
                            let mut max_time = last_time;
                            for (url, title, visit_time) in rows {
                                if Self::is_domain_excluded(&url, &exclude_domains) {
                                    continue;
                                }
                                if visit_time > max_time {
                                    max_time = visit_time;
                                }
                                yield RawEvent::new(
                                    "browser_history".to_string(),
                                    serde_json::json!({
                                        "url": url,
                                        "title": title,
                                        "browser": name,
                                        "visit_time": visit_time,
                                    }),
                                );
                            }
                            if max_time > last_time {
                                offsets.insert(name.to_string(), max_time);
                                let tracker = OffsetTracker::new(
                                    format!("browser_history_{}", name),
                                    tracker_db_path.clone(),
                                );
                                let _ = tracker.save_offset(max_time.to_string()).await;
                            }
                        }
                        Err(e) => {
                            tracing::debug!("Failed to read {} history: {}", name, e);
                        }
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
    async fn stop(&self) -> anyhow::Result<()> { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_creation() {
        let c = BrowserHistoryCollector::new(PathBuf::from(":memory:"));
        assert_eq!(c.name(), "browser_history");
    }

    #[test]
    fn test_domain_exclusion() {
        let excl = vec!["*.bank.com".to_string(), "secret.example.org".to_string()];
        assert!(BrowserHistoryCollector::is_domain_excluded(
            "https://www.bank.com/login", &excl));
        assert!(BrowserHistoryCollector::is_domain_excluded(
            "https://secret.example.org/page", &excl));
        assert!(!BrowserHistoryCollector::is_domain_excluded(
            "https://github.com/project", &excl));
    }
}
