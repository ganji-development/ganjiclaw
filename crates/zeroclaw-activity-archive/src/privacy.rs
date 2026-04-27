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
                        // Match against the host, not the full URL — otherwise
                        // `*.bank.com` never matches `https://www.bank.com/login`.
                        if let Some(host) = url::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_string)) {
                            if self.matches_pattern(&host, &rule.pattern) {
                                return true;
                            }
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
    ///
    /// These cover common sensitive contexts:
    /// - Password manager windows
    /// - SSH/GPG key paths
    /// - Environment files with secrets
    /// - Incognito/private browsing
    /// - Banking and financial domains
    pub fn default_rules() -> Vec<PrivacyRule> {
        vec![
            // Path exclusions — files that routinely contain secrets
            PrivacyRule::new(
                PrivacyRuleType::ExcludePath,
                "**/.ssh/**".to_string(),
                PrivacyAction::Exclude,
            ),
            PrivacyRule::new(
                PrivacyRuleType::ExcludePath,
                "**/.gnupg/**".to_string(),
                PrivacyAction::Exclude,
            ),
            PrivacyRule::new(
                PrivacyRuleType::ExcludePath,
                "**/.env".to_string(),
                PrivacyAction::Exclude,
            ),
            PrivacyRule::new(
                PrivacyRuleType::ExcludePath,
                "**/.env.*".to_string(),
                PrivacyAction::Exclude,
            ),
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
            // Title exclusions — password managers and sensitive windows
            PrivacyRule::new(
                PrivacyRuleType::ExcludeTitle,
                "*password*".to_string(),
                PrivacyAction::Exclude,
            ),
            PrivacyRule::new(
                PrivacyRuleType::ExcludeTitle,
                "*1Password*".to_string(),
                PrivacyAction::Exclude,
            ),
            PrivacyRule::new(
                PrivacyRuleType::ExcludeTitle,
                "*KeePass*".to_string(),
                PrivacyAction::Exclude,
            ),
            PrivacyRule::new(
                PrivacyRuleType::ExcludeTitle,
                "*Bitwarden*".to_string(),
                PrivacyAction::Exclude,
            ),
            PrivacyRule::new(
                PrivacyRuleType::ExcludeTitle,
                "*LastPass*".to_string(),
                PrivacyAction::Exclude,
            ),
            PrivacyRule::new(
                PrivacyRuleType::ExcludeTitle,
                "*InPrivate*".to_string(),
                PrivacyAction::Exclude,
            ),
            PrivacyRule::new(
                PrivacyRuleType::ExcludeTitle,
                "*Incognito*".to_string(),
                PrivacyAction::Exclude,
            ),
            PrivacyRule::new(
                PrivacyRuleType::ExcludeTitle,
                "*Private Browsing*".to_string(),
                PrivacyAction::Exclude,
            ),
            // Domain exclusions — banking and financial sites
            PrivacyRule::new(
                PrivacyRuleType::ExcludeDomain,
                "*.bank.com".to_string(),
                PrivacyAction::Exclude,
            ),
            PrivacyRule::new(
                PrivacyRuleType::ExcludeDomain,
                "*.bankofamerica.com".to_string(),
                PrivacyAction::Exclude,
            ),
            PrivacyRule::new(
                PrivacyRuleType::ExcludeDomain,
                "*.chase.com".to_string(),
                PrivacyAction::Exclude,
            ),
            PrivacyRule::new(
                PrivacyRuleType::ExcludeDomain,
                "*.paypal.com".to_string(),
                PrivacyAction::Exclude,
            ),
            PrivacyRule::new(
                PrivacyRuleType::ExcludeDomain,
                "*.venmo.com".to_string(),
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

    /// Check if a value matches a pattern. Case-insensitive.
    fn matches_pattern(&self, value: &str, pattern: &str) -> bool {
        if pattern.contains('*') || pattern.contains('?') {
            let regex_pattern = pattern
                .replace('.', r"\.")
                .replace('*', ".*")
                .replace('?', ".");
            // (?i) — privacy patterns must match regardless of case;
            // "*password*" needs to catch "Password" and "PASSWORD" too.
            if let Ok(re) = regex::Regex::new(&format!("(?i)^{}$", regex_pattern)) {
                return re.is_match(value);
            }
        }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{init_schema, Event, EventType};
    use rusqlite::Connection;

    fn setup_manager() -> PrivacyManager {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        PrivacyManager::new(Arc::new(Mutex::new(conn)))
    }

    #[test]
    fn test_add_list_remove_rule() {
        let pm = setup_manager();

        pm.add_exclusion(
            PrivacyRuleType::ExcludePath,
            "**/test/**".to_string(),
            PrivacyAction::Exclude,
        )
        .unwrap();

        let rules = pm.list_rules().unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].pattern, "**/test/**");

        let id = rules[0].id.clone();
        pm.remove_rule(&id).unwrap();
        assert!(pm.list_rules().unwrap().is_empty());
    }

    #[test]
    fn test_should_exclude_path() {
        let pm = setup_manager();
        pm.add_exclusion(
            PrivacyRuleType::ExcludePath,
            "**/passwords/**".to_string(),
            PrivacyAction::Exclude,
        )
        .unwrap();

        let mut event = Event::new("file_activity".to_string(), EventType::FileCreate);
        event.path = Some("/home/user/passwords/secret.txt".to_string());
        assert!(pm.should_exclude(&event));
    }

    #[test]
    fn test_should_exclude_title() {
        let pm = setup_manager();
        pm.add_exclusion(
            PrivacyRuleType::ExcludeTitle,
            "*password*".to_string(),
            PrivacyAction::Exclude,
        )
        .unwrap();

        let mut event = Event::new("window_focus".to_string(), EventType::WindowFocus);
        event.title = Some("Enter Password".to_string());
        assert!(pm.should_exclude(&event));
    }

    #[test]
    fn test_should_exclude_domain() {
        let pm = setup_manager();
        pm.add_exclusion(
            PrivacyRuleType::ExcludeDomain,
            "*.bank.com".to_string(),
            PrivacyAction::Exclude,
        )
        .unwrap();

        // Domain rules match the URL host, not the full URL — and they're
        // case-insensitive. Both behaviors are part of the privacy contract.
        let mut event = Event::new("browser_visit".to_string(), EventType::BrowserVisit);
        event.details = serde_json::json!({ "url": "https://www.BANK.com/login" });
        assert!(pm.should_exclude(&event));

        // Same TLD, different domain — must NOT match.
        let mut other = Event::new("browser_visit".to_string(), EventType::BrowserVisit);
        other.details = serde_json::json!({ "url": "https://example.com/" });
        assert!(!pm.should_exclude(&other));
    }

    #[test]
    fn test_does_not_exclude_safe_events() {
        let pm = setup_manager();
        let mut event = Event::new("window_focus".to_string(), EventType::WindowFocus);
        event.title = Some("Safe Document".to_string());
        event.path = Some("/home/user/documents/work.txt".to_string());
        assert!(!pm.should_exclude(&event));
    }

    #[test]
    fn test_redact_replaces_title() {
        let pm = setup_manager();
        pm.add_exclusion(
            PrivacyRuleType::Redaction,
            "*secret*".to_string(),
            PrivacyAction::Redact,
        )
        .unwrap();

        let mut event = Event::new("window_focus".to_string(), EventType::WindowFocus);
        event.title = Some("My Secret Document".to_string());
        pm.redact(&mut event);
        assert_eq!(event.title.as_deref(), Some("[REDACTED]"));
    }

    #[test]
    fn test_hash_replaces_title_with_hex() {
        let pm = setup_manager();
        pm.add_exclusion(
            PrivacyRuleType::Redaction,
            "*token*".to_string(),
            PrivacyAction::Hash,
        )
        .unwrap();

        let mut event = Event::new("window_focus".to_string(), EventType::WindowFocus);
        let original = "My Token Value".to_string();
        event.title = Some(original.clone());
        pm.redact(&mut event);

        let hashed = event.title.unwrap();
        assert_ne!(hashed, original);
        assert!(hashed.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_default_rules_cover_common_secrets() {
        let rules = PrivacyManager::default_rules();
        assert!(!rules.is_empty());

        // Spot-check the categories that matter for the privacy contract.
        assert!(rules.iter().any(|r| r.rule_type == PrivacyRuleType::ExcludePath
            && r.pattern.contains("passwords")));
        assert!(rules.iter().any(|r| r.rule_type == PrivacyRuleType::ExcludeTitle
            && r.pattern.contains("password")));
        assert!(rules.iter().any(|r| r.rule_type == PrivacyRuleType::ExcludeDomain
            && r.pattern.contains("bank")));
    }

    #[test]
    fn test_initialize_default_rules_seeds_db() {
        let pm = setup_manager();
        pm.initialize_default_rules().unwrap();
        let rules = pm.list_rules().unwrap();
        assert_eq!(rules.len(), PrivacyManager::default_rules().len());

        // Idempotent — running again does not duplicate rows.
        pm.initialize_default_rules().unwrap();
        assert_eq!(pm.list_rules().unwrap().len(), PrivacyManager::default_rules().len());
    }
}
