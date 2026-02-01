//! Configuration module for the Telegram translation bot.
//!
//! Supports loading configuration from both environment variables and TOML files.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Main configuration structure for the bot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Telegram API configuration
    pub telegram: TelegramConfig,
    /// Translation service configuration
    pub translation: TranslationConfig,
    /// Bot behavior settings
    #[serde(default)]
    pub bot: BotConfig,
}

/// Telegram API credentials
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramConfig {
    /// API ID from my.telegram.org
    pub api_id: i32,
    /// API Hash from my.telegram.org
    pub api_hash: String,
    /// Bot token from @BotFather (optional, for bot mode)
    pub bot_token: Option<String>,
    /// Phone number for user mode (optional)
    pub phone_number: Option<String>,
    /// Session file path
    #[serde(default = "default_session_path")]
    pub session_file: String,
}

fn default_session_path() -> String {
    "bot.session".to_string()
}

/// Translation service configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslationConfig {
    /// Translation provider (deepl, google, libretranslate)
    #[serde(default = "default_provider")]
    pub provider: TranslationProvider,
    /// API key for the translation service
    pub api_key: Option<String>,
    /// API endpoint URL (for self-hosted services like LibreTranslate)
    pub api_url: Option<String>,
    /// Default target language for translations
    #[serde(default = "default_target_language")]
    pub default_target_language: String,
}

fn default_provider() -> TranslationProvider {
    TranslationProvider::MyMemory
}

fn default_target_language() -> String {
    "en".to_string()
}

/// Supported translation providers
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TranslationProvider {
    DeepL,
    Google,
    LibreTranslate,
    MyMemory,
}

/// Bot behavior configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotConfig {
    /// Command prefix for bot commands
    #[serde(default = "default_command_prefix")]
    pub command_prefix: String,
    /// Whether to show the original message alongside translation
    #[serde(default = "default_show_original")]
    pub show_original: bool,
    /// List of admin user IDs who can configure the bot
    #[serde(default)]
    pub admin_user_ids: Vec<i64>,
    /// Reply format template
    #[serde(default = "default_reply_format")]
    pub reply_format: String,
}

fn default_command_prefix() -> String {
    "/".to_string()
}

fn default_show_original() -> bool {
    false
}

fn default_reply_format() -> String {
    "🌐 **Translation** ({source_lang} → {target_lang}):\n{translation}".to_string()
}

impl Default for BotConfig {
    fn default() -> Self {
        Self {
            command_prefix: default_command_prefix(),
            show_original: default_show_original(),
            admin_user_ids: Vec::new(),
            reply_format: default_reply_format(),
        }
    }
}

/// Runtime state for translation targets (persisted separately)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TranslationTargets {
    /// Map of group ID -> set of user IDs to translate
    pub groups: HashMap<i64, GroupTranslationConfig>,
}

/// Per-group translation configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GroupTranslationConfig {
    /// Users whose messages should be translated
    pub users: HashSet<i64>,
    /// Target languages for this group (overrides default)
    #[serde(default)]
    pub target_languages: Vec<String>,
    /// Whether translation is enabled for this group
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

/// Thread-safe wrapper for translation targets
pub type SharedTranslationTargets = Arc<RwLock<TranslationTargets>>;

impl Config {
    /// Load configuration from a TOML file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = std::fs::read_to_string(path.as_ref())
            .with_context(|| format!("Failed to read config file: {:?}", path.as_ref()))?;
        let config: Config =
            toml::from_str(&content).with_context(|| "Failed to parse config file")?;
        Ok(config)
    }

    /// Load configuration from environment variables
    pub fn from_env() -> Result<Self> {
        // Load .env file if present
        let _ = dotenvy::dotenv();

        let api_id: i32 = std::env::var("TG_API_ID")
            .context("TG_API_ID environment variable not set")?
            .parse()
            .context("TG_API_ID must be a valid integer")?;

        let api_hash = std::env::var("TG_API_HASH").context("TG_API_HASH not set")?;

        let bot_token = std::env::var("TG_BOT_TOKEN").ok();
        let phone_number = std::env::var("TG_PHONE_NUMBER").ok();

        let session_file = std::env::var("TG_SESSION_FILE").unwrap_or_else(|_| "bot.session".into());

        let provider = match std::env::var("TRANSLATION_PROVIDER")
            .unwrap_or_else(|_| "mymemory".into())
            .to_lowercase()
            .as_str()
        {
            "deepl" => TranslationProvider::DeepL,
            "google" => TranslationProvider::Google,
            "libretranslate" => TranslationProvider::LibreTranslate,
            _ => TranslationProvider::MyMemory,
        };

        let translation_api_key = std::env::var("TRANSLATION_API_KEY").ok();
        let translation_api_url = std::env::var("TRANSLATION_API_URL").ok();
        let default_target_language =
            std::env::var("DEFAULT_TARGET_LANGUAGE").unwrap_or_else(|_| "en".into());

        let admin_user_ids: Vec<i64> = std::env::var("ADMIN_USER_IDS")
            .unwrap_or_default()
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();

        Ok(Config {
            telegram: TelegramConfig {
                api_id,
                api_hash,
                bot_token,
                phone_number,
                session_file,
            },
            translation: TranslationConfig {
                provider,
                api_key: translation_api_key,
                api_url: translation_api_url,
                default_target_language,
            },
            bot: BotConfig {
                admin_user_ids,
                ..Default::default()
            },
        })
    }

    /// Load configuration, trying file first, then environment
    pub fn load() -> Result<Self> {
        // Try config.toml first
        if Path::new("config.toml").exists() {
            return Self::from_file("config.toml");
        }

        // Fall back to environment variables
        Self::from_env()
    }
}

impl TranslationTargets {
    /// Load translation targets from a JSON file
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        if !path.as_ref().exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(path.as_ref())
            .with_context(|| format!("Failed to read targets file: {:?}", path.as_ref()))?;
        let targets: TranslationTargets =
            serde_json::from_str(&content).with_context(|| "Failed to parse targets file")?;
        Ok(targets)
    }

    /// Save translation targets to a JSON file
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path.as_ref(), content)
            .with_context(|| format!("Failed to write targets file: {:?}", path.as_ref()))?;
        Ok(())
    }

    /// Check if a user in a group should be translated
    pub fn should_translate(&self, group_id: i64, user_id: i64) -> bool {
        self.groups
            .get(&group_id)
            .map(|g| g.enabled && g.users.contains(&user_id))
            .unwrap_or(false)
    }

    /// Get target languages for a group
    pub fn get_target_languages(&self, group_id: i64, default: &str) -> Vec<String> {
        self.groups
            .get(&group_id)
            .map(|g| {
                if g.target_languages.is_empty() {
                    vec![default.to_string()]
                } else {
                    g.target_languages.clone()
                }
            })
            .unwrap_or_else(|| vec![default.to_string()])
    }

    /// Add a user to translate in a group
    pub fn add_user(&mut self, group_id: i64, user_id: i64) {
        self.groups
            .entry(group_id)
            .or_insert_with(|| GroupTranslationConfig {
                enabled: true,
                ..Default::default()
            })
            .users
            .insert(user_id);
    }

    /// Remove a user from translation in a group
    pub fn remove_user(&mut self, group_id: i64, user_id: i64) -> bool {
        if let Some(group) = self.groups.get_mut(&group_id) {
            return group.users.remove(&user_id);
        }
        false
    }

    /// Set target languages for a group
    pub fn set_target_languages(&mut self, group_id: i64, languages: Vec<String>) {
        self.groups
            .entry(group_id)
            .or_insert_with(|| GroupTranslationConfig {
                enabled: true,
                ..Default::default()
            })
            .target_languages = languages;
    }

    /// Enable/disable translation for a group
    pub fn set_enabled(&mut self, group_id: i64, enabled: bool) {
        if let Some(group) = self.groups.get_mut(&group_id) {
            group.enabled = enabled;
        }
    }

    /// Get list of users being translated in a group
    pub fn list_users(&self, group_id: i64) -> Vec<i64> {
        self.groups
            .get(&group_id)
            .map(|g| g.users.iter().copied().collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_translation_targets() {
        let mut targets = TranslationTargets::default();

        // Add a user
        targets.add_user(123, 456);
        assert!(targets.should_translate(123, 456));
        assert!(!targets.should_translate(123, 789));
        assert!(!targets.should_translate(999, 456));

        // Remove user
        assert!(targets.remove_user(123, 456));
        assert!(!targets.should_translate(123, 456));

        // Set languages
        targets.set_target_languages(123, vec!["de".to_string(), "uk".to_string()]);
        assert_eq!(targets.get_target_languages(123, "en"), vec!["de", "uk"]);
        assert_eq!(targets.get_target_languages(999, "en"), vec!["en"]);
    }
}
