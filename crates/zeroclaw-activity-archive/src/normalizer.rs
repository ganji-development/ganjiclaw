//! Normalization pipeline for raw events.
//!
//! Converts raw events from collectors into canonical events,
//! applies privacy rules, and extracts entities.

use crate::schema::{Event, EventType, RawEvent, PrivacyRule, PrivacyRuleType, PrivacyAction};
use parking_lot::{Mutex, RwLock};
use rusqlite::{Connection, params};
use std::sync::Arc;
use anyhow::Result;

/// Normalizer for processing raw events.
pub struct Normalizer {
    db: Arc<Mutex<Connection>>,
    privacy_rules: Arc<RwLock<Vec<PrivacyRule>>>,
}

impl Normalizer {
    /// Create a new normalizer.
    pub fn new(db: Arc<Mutex<Connection>>) -> Self {
        Self {
            db,
            privacy_rules: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Load privacy rules from the database.
    pub fn load_privacy_rules(&self) -> Result<()> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare(
            "SELECT id, rule_type, pattern, action, created_at FROM privacy_rules"
        )?;

        let rules = stmt.query_map([], |row| {
            Ok(PrivacyRule {
                id: row.get(0)?,
                rule_type: PrivacyRuleType::from_str(&row.get::<_, String>(1)?)
                    .unwrap_or(PrivacyRuleType::ExcludePath),
                pattern: row.get(2)?,
                action: PrivacyAction::from_str(&row.get::<_, String>(3)?)
                    .unwrap_or(PrivacyAction::Exclude),
                created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(4)?)
                    .unwrap()
                    .with_timezone(&chrono::Utc),
            })
        })?.collect::<Result<Vec<_>, _>>()?;

        *self.privacy_rules.write() = rules;
        Ok(())
    }

    /// Process a raw event into a canonical event.
    pub fn process_raw_event(&self, raw: &RawEvent) -> Result<Option<Event>> {
        // 1. Parse source-specific payload
        let mut event = self.parse_raw_event(raw)?;

        // 2. Apply privacy rules
        if self.should_exclude(&event) {
            return Ok(None);
        }

        self.apply_privacy_rules(&mut event);

        // 3. Extract entities
        let entities = self.extract_entities(&event);

        // 4. Infer project_key from path/title
        event.project_key = self.infer_project_key(&event);

        // 5. Generate hash for deduplication
        event.hash = Some(event.generate_hash());

        // 6. Store in database
        self.store_event(&event)?;

        // 7. Update entity relationships
        for entity in entities {
            self.store_entity(&entity)?;
            self.link_event_to_entity(&event.id, &entity.id)?;
        }

        Ok(Some(event))
    }

    /// Parse a raw event into a canonical event.
    fn parse_raw_event(&self, raw: &RawEvent) -> Result<Event> {
        let event_type = match raw.source.as_str() {
            "window_focus" => EventType::WindowFocus,
            "process_launch" => EventType::ProcessStart,
            "browser_history" => EventType::BrowserVisit,
            "shell_activity" => EventType::ShellCommand,
            "file_activity" => {
                // Determine file event type from payload
                if let Some(action) = raw.payload.get("action").and_then(|v| v.as_str()) {
                    match action {
                        "create" => EventType::FileCreate,
                        "modify" => EventType::FileModify,
                        "delete" => EventType::FileDelete,
                        "rename" => EventType::FileRename,
                        _ => EventType::SystemEvent,
                    }
                } else {
                    EventType::SystemEvent
                }
            }
            _ => EventType::SystemEvent,
        };

        let mut event = Event::new(raw.source.clone(), event_type);
        event.ts_utc = raw.timestamp;
        event.ts_local = chrono::Local::now();

        // Extract common fields from payload
        if let Some(window_title) = raw.payload.get("window_title").and_then(|v| v.as_str()) {
            event.title = Some(window_title.to_string());
        }

        if let Some(process_name) = raw.payload.get("process_name").and_then(|v| v.as_str()) {
            event.app = Some(process_name.to_string());
        }

        if let Some(path) = raw.payload.get("path").and_then(|v| v.as_str()) {
            event.path = Some(path.to_string());
        }

        if let Some(url) = raw.payload.get("url").and_then(|v| v.as_str()) {
            event.details = serde_json::json!({ "url": url });
        }

        if let Some(command) = raw.payload.get("command").and_then(|v| v.as_str()) {
            event.details = serde_json::json!({ "command": command });
        }

        event.raw_ref = Some(raw.id.clone());

        Ok(event)
    }

    /// Check if event should be excluded based on privacy rules.
    fn should_exclude(&self, event: &Event) -> bool {
        let rules = self.privacy_rules.read();

        for rule in rules.iter() {
            match rule.rule_type {
                PrivacyRuleType::ExcludePath => {
                    if let Some(path) = &event.path {
                        if self.matches_pattern(path, &rule.pattern) {
                            return true;
                        }
                    }
                }
                PrivacyRuleType::ExcludeTitle => {
                    if let Some(title) = &event.title {
                        if self.matches_pattern(title, &rule.pattern) {
                            return true;
                        }
                    }
                }
                PrivacyRuleType::ExcludeDomain => {
                    if let Some(url) = event.details.get("url").and_then(|v| v.as_str()) {
                        if self.matches_pattern(url, &rule.pattern) {
                            return true;
                        }
                    }
                }
                PrivacyRuleType::Redaction => {
                    // Redaction rules don't exclude, they modify
                }
            }
        }

        false
    }

    /// Apply privacy rules to event data.
    fn apply_privacy_rules(&self, event: &mut Event) {
        let rules = self.privacy_rules.read();

        for rule in rules.iter() {
            if rule.rule_type == PrivacyRuleType::Redaction {
                match rule.action {
                    PrivacyAction::Redact => {
                        // Redact sensitive fields
                        if let Some(title) = &event.title {
                            if self.matches_pattern(title, &rule.pattern) {
                                event.title = Some("[REDACTED]".to_string());
                            }
                        }
                    }
                    PrivacyAction::Hash => {
                        // Hash sensitive fields
                        if let Some(title) = &event.title {
                            if self.matches_pattern(title, &rule.pattern) {
                                event.title = Some(self.hash_value(title));
                            }
                        }
                    }
                    PrivacyAction::Exclude => {
                        // Already handled in should_exclude
                    }
                }
            }
        }
    }

    /// Extract entities from event.
    fn extract_entities(&self, event: &Event) -> Vec<crate::schema::Entity> {
        let mut entities = Vec::new();

        // Extract app entity
        if let Some(app) = &event.app {
            entities.push(crate::schema::Entity::new(
                crate::schema::EntityType::App,
                app.clone(),
            ));
        }

        // Extract project from path
        if let Some(path) = &event.path {
            if let Some(project_name) = self.extract_project_from_path(path) {
                entities.push(crate::schema::Entity::new(
                    crate::schema::EntityType::Project,
                    project_name,
                ));
            }
        }

        // Extract domain from URL
        if let Some(url) = event.details.get("url").and_then(|v| v.as_str()) {
            if let Some(domain) = self.extract_domain_from_url(url) {
                entities.push(crate::schema::Entity::new(
                    crate::schema::EntityType::Domain,
                    domain,
                ));
            }
        }

        entities
    }

    /// Infer project key from event.
    fn infer_project_key(&self, event: &Event) -> Option<String> {
        // Try to extract from path first
        if let Some(path) = &event.path {
            if let Some(project) = self.extract_project_from_path(path) {
                return Some(project);
            }
        }

        // Try to extract from title
        if let Some(title) = &event.title {
            if let Some(project) = self.extract_project_from_title(title) {
                return Some(project);
            }
        }

        None
    }

    /// Extract project name from file path.
    fn extract_project_from_path(&self, path: &str) -> Option<String> {
        // Look for common project indicators in path
        let path_lower = path.to_lowercase();

        // Check for git repo
        if path_lower.contains(".git") || path_lower.contains("src") {
            // Extract parent directory name
            if let Some(parent) = std::path::Path::new(path).parent() {
                if let Some(name) = parent.file_name() {
                    if let Some(name_str) = name.to_str() {
                        return Some(name_str.to_string());
                    }
                }
            }
        }

        None
    }

    /// Extract project name from window title.
    fn extract_project_from_title(&self, title: &str) -> Option<String> {
        // Look for common patterns in titles
        // This is a simplified implementation
        if title.contains(" - ") {
            let parts: Vec<&str> = title.split(" - ").collect();
            if parts.len() >= 2 {
                return Some(parts[0].trim().to_string());
            }
        }

        None
    }

    /// Extract domain from URL.
    fn extract_domain_from_url(&self, url: &str) -> Option<String> {
        if let Ok(parsed) = url::Url::parse(url) {
            if let Some(host) = parsed.host_str() {
                return Some(host.to_string());
            }
        }
        None
    }

    /// Check if a value matches a pattern.
    fn matches_pattern(&self, value: &str, pattern: &str) -> bool {
        // Support glob patterns
        if pattern.contains('*') {
            let regex_pattern = pattern
                .replace('.', r"\.")
                .replace('*', ".*")
                .replace('?', ".");
            if let Ok(re) = regex::Regex::new(&format!("^{}$", regex_pattern)) {
                return re.is_match(value);
            }
        }

        // Exact match
        value == pattern
    }

    /// Hash a value for privacy.
    fn hash_value(&self, value: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }

    /// Store an event in the database.
    fn store_event(&self, event: &Event) -> Result<()> {
        let conn = self.db.lock();

        conn.execute(
            "INSERT INTO events (
                id, ts_utc, ts_local, source, event_type, actor, host, app, title, path,
                details_json, sensitivity, project_key, session_id, hash, raw_ref, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            params![
                event.id,
                event.ts_utc.to_rfc3339(),
                event.ts_local.to_rfc3339(),
                event.source,
                event.event_type.as_str(),
                event.actor,
                event.host,
                event.app,
                event.title,
                event.path,
                serde_json::to_string(&event.details)?,
                event.sensitivity,
                event.project_key,
                event.session_id,
                event.hash,
                event.raw_ref,
                event.created_at.to_rfc3339(),
            ],
        )?;

        Ok(())
    }

    /// Store an entity in the database.
    fn store_entity(&self, entity: &crate::schema::Entity) -> Result<()> {
        let conn = self.db.lock();

        conn.execute(
            "INSERT OR REPLACE INTO entities (
                id, entity_type, name, metadata_json, first_seen, last_seen, occurrence_count
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                entity.id,
                entity.entity_type.as_str(),
                entity.name,
                serde_json::to_string(&entity.metadata)?,
                entity.first_seen.to_rfc3339(),
                entity.last_seen.to_rfc3339(),
                entity.occurrence_count,
            ],
        )?;

        Ok(())
    }

    /// Link an event to an entity.
    fn link_event_to_entity(&self, event_id: &str, entity_id: &str) -> Result<()> {
        let conn = self.db.lock();

        conn.execute(
            "INSERT OR IGNORE INTO event_entity_map (event_id, entity_id, relationship_type)
             VALUES (?1, ?2, ?3)",
            params![event_id, entity_id, "primary"],
        )?;

        Ok(())
    }
}
