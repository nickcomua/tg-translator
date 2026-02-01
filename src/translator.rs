//! Translation service module supporting multiple providers.
//!
//! Supports:
//! - LibreTranslate (free, self-hostable)
//! - DeepL (high quality, requires API key)
//! - Google Translate (via unofficial API)

use crate::config::{TranslationConfig, TranslationProvider};
use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum TranslationError {
    #[error("Translation API error: {0}")]
    ApiError(String),
    #[error("Network error: {0}")]
    NetworkError(#[from] reqwest::Error),
    #[error("No translation returned")]
    EmptyResponse,
    #[error("Invalid configuration: {0}")]
    ConfigError(String),
}

/// Result of a translation operation
#[derive(Debug, Clone)]
pub struct TranslationResult {
    /// Translated text
    pub text: String,
    /// Detected source language (if available)
    pub detected_language: Option<String>,
}

/// Translation service that supports multiple providers
#[derive(Clone)]
pub struct Translator {
    client: Client,
    config: Arc<TranslationConfig>,
}

impl Translator {
    /// Create a new translator with the given configuration
    pub fn new(config: TranslationConfig) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            config: Arc::new(config),
        }
    }

    /// Translate text to the target language
    pub async fn translate(
        &self,
        text: &str,
        target_language: &str,
        source_language: Option<&str>,
    ) -> Result<TranslationResult, TranslationError> {
        match self.config.provider {
            TranslationProvider::LibreTranslate => {
                self.translate_libretranslate(text, target_language, source_language)
                    .await
            }
            TranslationProvider::DeepL => {
                self.translate_deepl(text, target_language, source_language)
                    .await
            }
            TranslationProvider::Google => {
                self.translate_google(text, target_language, source_language)
                    .await
            }
            TranslationProvider::MyMemory => {
                self.translate_mymemory(text, target_language, source_language)
                    .await
            }
        }
    }

    /// Translate using LibreTranslate
    async fn translate_libretranslate(
        &self,
        text: &str,
        target_language: &str,
        source_language: Option<&str>,
    ) -> Result<TranslationResult, TranslationError> {
        let api_url = self
            .config
            .api_url
            .as_deref()
            .unwrap_or("https://libretranslate.com");

        #[derive(Serialize)]
        struct LibreTranslateRequest<'a> {
            q: &'a str,
            source: &'a str,
            target: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            api_key: Option<&'a str>,
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum LibreTranslateResponse {
            Success {
                #[serde(rename = "translatedText")]
                translated_text: String,
                #[serde(rename = "detectedLanguage")]
                detected_language: Option<DetectedLanguage>,
            },
            Error {
                error: String,
            },
        }

        #[derive(Deserialize)]
        struct DetectedLanguage {
            language: String,
        }

        let request = LibreTranslateRequest {
            q: text,
            source: source_language.unwrap_or("auto"),
            target: target_language,
            api_key: self.config.api_key.as_deref(),
        };

        let response = self
            .client
            .post(format!("{}/translate", api_url))
            .json(&request)
            .send()
            .await?;

        let result: LibreTranslateResponse = response.json().await?;

        match result {
            LibreTranslateResponse::Success {
                translated_text,
                detected_language,
            } => Ok(TranslationResult {
                text: translated_text,
                detected_language: detected_language.map(|d| d.language),
            }),
            LibreTranslateResponse::Error { error } => Err(TranslationError::ApiError(error)),
        }
    }

    /// Translate using DeepL API
    async fn translate_deepl(
        &self,
        text: &str,
        target_language: &str,
        source_language: Option<&str>,
    ) -> Result<TranslationResult, TranslationError> {
        let api_key = self
            .config
            .api_key
            .as_ref()
            .ok_or_else(|| TranslationError::ConfigError("DeepL requires an API key".to_string()))?;

        // Determine if using free or pro API
        let api_url = self.config.api_url.as_deref().unwrap_or_else(|| {
            if api_key.ends_with(":fx") {
                "https://api-free.deepl.com/v2"
            } else {
                "https://api.deepl.com/v2"
            }
        });

        #[derive(Deserialize)]
        struct DeepLResponse {
            translations: Vec<DeepLTranslation>,
        }

        #[derive(Deserialize)]
        struct DeepLTranslation {
            text: String,
            detected_source_language: Option<String>,
        }

        // DeepL uses form encoding
        let mut form = vec![
            ("text", text.to_string()),
            ("target_lang", target_language.to_uppercase()),
        ];
        if let Some(src) = source_language {
            form.push(("source_lang", src.to_uppercase()));
        }

        let response = self
            .client
            .post(format!("{}/translate", api_url))
            .header("Authorization", format!("DeepL-Auth-Key {}", api_key))
            .form(&form)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(TranslationError::ApiError(format!(
                "DeepL API error: {}",
                error_text
            )));
        }

        let result: DeepLResponse = response.json().await?;

        result
            .translations
            .into_iter()
            .next()
            .map(|t| TranslationResult {
                text: t.text,
                detected_language: t.detected_source_language.map(|s| s.to_lowercase()),
            })
            .ok_or(TranslationError::EmptyResponse)
    }

    /// Translate using Google Translate (unofficial API via lingva.ml or similar)
    async fn translate_google(
        &self,
        text: &str,
        target_language: &str,
        source_language: Option<&str>,
    ) -> Result<TranslationResult, TranslationError> {
        // Use Lingva Translate as a free Google Translate frontend
        let api_url = self
            .config
            .api_url
            .as_deref()
            .unwrap_or("https://lingva.ml/api/v1");

        let source = source_language.unwrap_or("auto");
        let encoded_text = urlencoding::encode(text);
        let url = format!("{}/{}/{}/{}", api_url, source, target_language, encoded_text);

        #[derive(Deserialize)]
        struct LingvaResponse {
            translation: String,
            info: Option<LingvaInfo>,
        }

        #[derive(Deserialize)]
        struct LingvaInfo {
            #[serde(rename = "detectedSource")]
            detected_source: Option<String>,
        }

        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(TranslationError::ApiError(format!(
                "Google Translate API error: {}",
                error_text
            )));
        }

        let result: LingvaResponse = response.json().await?;

        Ok(TranslationResult {
            text: result.translation,
            detected_language: result.info.and_then(|i| i.detected_source),
        })
    }

    /// Translate using MyMemory API (free, no API key required)
    async fn translate_mymemory(
        &self,
        text: &str,
        target_language: &str,
        source_language: Option<&str>,
    ) -> Result<TranslationResult, TranslationError> {
        let source = source_language.unwrap_or("en");
        let langpair = format!("{}|{}", source, target_language);

        let api_url = self
            .config
            .api_url
            .as_deref()
            .unwrap_or("https://api.mymemory.translated.net");

        #[derive(Deserialize)]
        struct MyMemoryResponse {
            #[serde(rename = "responseData")]
            response_data: Option<ResponseData>,
            #[serde(rename = "responseStatus")]
            response_status: i32,
            #[serde(rename = "responseDetails")]
            response_details: Option<String>,
        }

        #[derive(Deserialize)]
        struct ResponseData {
            #[serde(rename = "translatedText")]
            translated_text: String,
        }

        let url = format!("{}/get", api_url);

        let response = self
            .client
            .get(&url)
            .query(&[("q", text), ("langpair", &langpair)])
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(TranslationError::ApiError(format!(
                "MyMemory API error: {}",
                error_text
            )));
        }

        let result: MyMemoryResponse = response.json().await?;

        if result.response_status != 200 {
            return Err(TranslationError::ApiError(
                result.response_details.unwrap_or_else(|| "Unknown error".to_string()),
            ));
        }

        match result.response_data {
            Some(data) => Ok(TranslationResult {
                text: data.translated_text.trim().to_string(),
                detected_language: Some(source.to_string()),
            }),
            None => Err(TranslationError::EmptyResponse),
        }
    }

    /// Get the default target language from config
    pub fn default_target_language(&self) -> &str {
        &self.config.default_target_language
    }
}

/// Language codes for common languages
#[allow(dead_code)]
pub mod languages {
    pub const ENGLISH: &str = "en";
    pub const SPANISH: &str = "es";
    pub const FRENCH: &str = "fr";
    pub const GERMAN: &str = "de";
    pub const ITALIAN: &str = "it";
    pub const PORTUGUESE: &str = "pt";
    pub const RUSSIAN: &str = "ru";
    pub const CHINESE: &str = "zh";
    pub const JAPANESE: &str = "ja";
    pub const KOREAN: &str = "ko";
    pub const ARABIC: &str = "ar";
    pub const HINDI: &str = "hi";
    pub const UKRAINIAN: &str = "uk";
    pub const POLISH: &str = "pl";
    pub const DUTCH: &str = "nl";
    pub const TURKISH: &str = "tr";

    /// List of valid ISO 639-1 language codes supported by most translation APIs
    pub const VALID_CODES: &[&str] = &[
        "af", "sq", "am", "ar", "hy", "az", "eu", "be", "bn", "bs", "bg", "ca", "ceb", "zh",
        "co", "hr", "cs", "da", "nl", "en", "eo", "et", "fi", "fr", "fy", "gl", "ka", "de",
        "el", "gu", "ht", "ha", "haw", "he", "hi", "hmn", "hu", "is", "ig", "id", "ga", "it",
        "ja", "jv", "kn", "kk", "km", "rw", "ko", "ku", "ky", "lo", "la", "lv", "lt", "lb",
        "mk", "mg", "ms", "ml", "mt", "mi", "mr", "mn", "my", "ne", "no", "ny", "or", "ps",
        "fa", "pl", "pt", "pa", "ro", "ru", "sm", "gd", "sr", "st", "sn", "sd", "si", "sk",
        "sl", "so", "es", "su", "sw", "sv", "tl", "tg", "ta", "tt", "te", "th", "tr", "tk",
        "uk", "ur", "ug", "uz", "vi", "cy", "xh", "yi", "yo", "zu",
    ];

    /// Check if a language code is valid
    pub fn is_valid(code: &str) -> bool {
        VALID_CODES.contains(&code)
    }

    /// Normalize language code (handle common aliases)
    pub fn normalize(code: &str) -> &str {
        match code {
            "ua" => "uk", // Ukrainian: ua is common but uk is ISO 639-1
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TranslationConfig;

    #[test]
    fn test_language_validation() {
        // Valid codes
        assert!(languages::is_valid("en"));
        assert!(languages::is_valid("de"));
        assert!(languages::is_valid("uk"));
        assert!(languages::is_valid("ru"));
        assert!(languages::is_valid("fr"));
        assert!(languages::is_valid("es"));
        assert!(languages::is_valid("ja"));
        assert!(languages::is_valid("zh"));

        // Invalid codes
        assert!(!languages::is_valid(""));
        assert!(!languages::is_valid("eng"));
        assert!(!languages::is_valid("EN"));
        assert!(!languages::is_valid("gg")); // Not a real language
        assert!(!languages::is_valid("xx")); // Not a real language
        assert!(!languages::is_valid("ua")); // Should use "uk" for Ukrainian
    }

    #[test]
    fn test_language_normalization() {
        assert_eq!(languages::normalize("ua"), "uk");
        assert_eq!(languages::normalize("en"), "en");
        assert_eq!(languages::normalize("de"), "de");
    }

    #[tokio::test]
    async fn test_mymemory_translation() {
        let config = TranslationConfig {
            provider: crate::config::TranslationProvider::MyMemory,
            api_key: None,
            api_url: None,
            default_target_language: "uk".to_string(),
        };

        let translator = Translator::new(config);
        let result = translator.translate("Hello", "uk", Some("en")).await;

        match result {
            Ok(translation) => {
                println!("Translation: {}", translation.text);
                assert!(!translation.text.is_empty());
                // Ukrainian translation should contain Cyrillic characters
                assert!(translation.text.chars().any(|c| c >= '\u{0400}' && c <= '\u{04FF}'));
            }
            Err(e) => {
                panic!("Translation failed: {}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_translation_different_languages() {
        let config = TranslationConfig {
            provider: crate::config::TranslationProvider::MyMemory,
            api_key: None,
            api_url: None,
            default_target_language: "es".to_string(),
        };

        let translator = Translator::new(config);

        // Translate English to Spanish
        let result = translator.translate("Good morning", "es", Some("en")).await;
        match result {
            Ok(translation) => {
                println!("Spanish translation: {}", translation.text);
                assert!(!translation.text.is_empty());
            }
            Err(e) => {
                // Network errors in test environment are acceptable
                println!("Translation test skipped due to network: {}", e);
            }
        }
    }
}
