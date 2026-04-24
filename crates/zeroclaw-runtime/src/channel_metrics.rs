//! Per-channel inbound-message metrics.
//!
//! Mirrors the [`crate::health`] registry: a lazily initialized global store
//! that the channel orchestrator writes into and the gateway reads from to
//! build the `/api/channels` response.

use chrono::Utc;
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

#[derive(Debug, Clone, Serialize)]
pub struct ChannelMetrics {
    pub channel_type: String,
    pub message_count: u64,
    pub last_message_at: Option<String>,
}

static REGISTRY: OnceLock<Mutex<BTreeMap<String, ChannelMetrics>>> = OnceLock::new();

fn registry() -> &'static Mutex<BTreeMap<String, ChannelMetrics>> {
    REGISTRY.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Register a running channel so it appears in snapshots even before it has
/// received any messages. Idempotent — re-registering preserves existing counts.
pub fn register(name: &str, channel_type: &str) {
    let mut map = registry().lock();
    map.entry(name.to_string())
        .or_insert_with(|| ChannelMetrics {
            channel_type: channel_type.to_string(),
            message_count: 0,
            last_message_at: None,
        });
}

/// Bump the inbound-message counter and timestamp for a channel.
/// Creates the entry if the channel wasn't pre-registered.
pub fn record_inbound(name: &str) {
    let mut map = registry().lock();
    let now = Utc::now().to_rfc3339();
    let entry = map
        .entry(name.to_string())
        .or_insert_with(|| ChannelMetrics {
            channel_type: name.to_string(),
            message_count: 0,
            last_message_at: None,
        });
    entry.message_count = entry.message_count.saturating_add(1);
    entry.last_message_at = Some(now);
}

pub fn snapshot() -> BTreeMap<String, ChannelMetrics> {
    registry().lock().clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique(prefix: &str) -> String {
        format!("{prefix}-{}", uuid::Uuid::new_v4())
    }

    #[test]
    fn register_creates_entry_with_zero_count() {
        let name = unique("metrics-register");
        register(&name, "test");
        let snap = snapshot();
        let entry = snap.get(&name).expect("entry present after register");
        assert_eq!(entry.channel_type, "test");
        assert_eq!(entry.message_count, 0);
        assert!(entry.last_message_at.is_none());
    }

    #[test]
    fn record_inbound_bumps_count_and_stamps_time() {
        let name = unique("metrics-record");
        register(&name, "test");
        record_inbound(&name);
        record_inbound(&name);
        let snap = snapshot();
        let entry = snap.get(&name).expect("entry present");
        assert_eq!(entry.message_count, 2);
        assert!(entry.last_message_at.is_some());
    }

    #[test]
    fn record_inbound_without_register_creates_entry() {
        let name = unique("metrics-autocreate");
        record_inbound(&name);
        let snap = snapshot();
        let entry = snap.get(&name).expect("entry present");
        assert_eq!(entry.message_count, 1);
        assert_eq!(entry.channel_type, name);
    }

    #[test]
    fn register_is_idempotent_and_preserves_counts() {
        let name = unique("metrics-idem");
        register(&name, "test");
        record_inbound(&name);
        register(&name, "test");
        let snap = snapshot();
        assert_eq!(snap.get(&name).expect("entry").message_count, 1);
    }
}
