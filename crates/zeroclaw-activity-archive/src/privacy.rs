//! Privacy controls for the activity archive.
//!
//! Manages exclusion rules and redaction policies.

use crate::schema::{PrivacyRule, PrivacyRuleType, PrivacyAction, Event};
use parking_lot::Mutex;
use rusqlite::{Connection, params};
use std::sync::Arc;
use anyhow::Result;

/// Privacy manager for controlling data collection and redaction.
pub struct PrivacyManager {
    db: Arc<Mutex<Connection>>,
}

impl PrivacyManager {
    /// Create a new privacy manager.
    pub fn new(db: Arc<Mutex<Connection>>) -> Self {
        Self { db }
    }

    /// Add an exclusion rule.
    pub fn add_exclusion(&self, rule_type: PrivacyRuleType, pattern: String, action: PrivacyAction) -> Result<()> {
        let rule = PrivacyRule::new(rule_type, pattern, action);
        self.store_rule(&rule)?;
        Ok(())
    }

    /// Remove a privacy rule.
    pub fn remove_rule(&self, id: &str) -> Result<()> {
        let conn = self.db.lock();

        conn.execute(
            "DELETE FROM privacy_rules WHERE id = ?1",
            params![id],
        )?;

        Ok(())
    }

    /// List all privacy rules.
    pub fn list_rules(&self) -> Result<Vec<PrivacyRule>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare(
            "SELECT id, rule_type, pattern, action, created_at FROM privacy_rules ORDER BY created_at DESC"
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

        Ok(rules)
    }

    /// Check if an event should be excluded.
    pub fn should_exclude(&self, event: &Event) -> bool {
        let rules = match self.list_rules() {
            Ok(r) => r,
            Err(_) => return false,
        };

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

    /// Apply redaction to sensitive fields.
    pub fn redact(&self, event: &mut Event) {
        let rules = match self.list_rules() {
            Ok(r) => r,
            Err(_) => return,
        };

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

    /// Get default privacy rules.
    pub fn default_rules() -> Vec<PrivacyRule> {
        vec![
            PrivacyRule::new(
                PrivacyRuleType::ExcludePath,
                "**/passwords/**".to_string(),
                PrivacyAction::Exclude,
            ),
            PrivacyRule::new(
                PrivacyRuleType::ExcludePath,
                "**/banking/**".to_string(),
                PrivacyAction::Exclude,
            ),
            PrivacyRule::new(
                PrivacyRuleType::ExcludeTitle,
                "*password*".to_string(),
                PrivacyAction::Exclude,
            ),
            PrivacyRule::new(
                PrivacyRuleType::ExcludeDomain,
                "*.bank.com".to_string(),
                PrivacyAction::Exclude,
            ),
        ]
    }

    /// Initialize default privacy rules.
    pub fn initialize_default_rules(&self) -> Result<()> {
        let existing_rules = self.list_rules()?;

        if existing_rules.is_empty() {
            for rule in Self::default_rules() {
                self.store_rule(&rule)?;
            }
        }

        Ok(())
    }

    /// Store a privacy rule in the database.
    fn store_rule(&self, rule: &PrivacyRule) -> Result<()> {
        let conn = self.db.lock();

        conn.execute(
            "INSERT INTO privacy_rules (id, rule_type, pattern, action, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                rule.id,
                rule.rule_type.as_str(),
                rule.pattern,
                rule.action.as_str(),
                rule.created_at.to_rfc3339(),
            ],
        )?;

        Ok(())
    }

    /// Check if a value matches a pattern.
    fn matches_pattern(&self, value: &str, pattern: &str) -> bool {
        // Support glob patterns
        if pattern.contains('*') || pattern.contains('?') {
            let regex_pattern = pattern
                .replace('.', r"\.")
                .replace('*', ".*")
                .replace('?', ".");
            if let Ok(re) = regex::Regex::new(&format!("^{}$", regex_pattern)) {
                return re.is_match(value);
            }
        }

        // Case-insensitive exact match
        value.to_lowercase() == pattern.to_lowercase()
    }

    /// Hash a value for privacy.
    fn hash_value(&self, value: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }
}
