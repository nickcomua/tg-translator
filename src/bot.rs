//! Bot logic and command handlers for the Telegram translation bot.

use crate::config::{BotConfig, SharedTranslationTargets};
use crate::translator::Translator;
use anyhow::{Context, Result};
use grammers_client::client::updates::UpdateStream;
use grammers_client::types::{Message, Peer};
use grammers_client::{Client, InputMessage, Update};
use grammers_session::defs::PeerKind;
use std::ops::Deref;
use tracing::{debug, error, info, warn};

/// Bot instance that handles messages and commands
pub struct Bot {
    #[allow(dead_code)]
    client: Client,
    translator: Translator,
    targets: SharedTranslationTargets,
    config: BotConfig,
    targets_file: String,
    #[allow(dead_code)]
    api_hash: String,
}

/// Parsed bot command
#[derive(Debug)]
struct Command {
    name: String,
    args: Vec<String>,
}

impl Bot {
    /// Create a new bot instance
    pub fn new(
        client: Client,
        translator: Translator,
        targets: SharedTranslationTargets,
        config: BotConfig,
        targets_file: String,
        api_hash: String,
    ) -> Self {
        Self {
            client,
            translator,
            targets,
            config,
            targets_file,
            api_hash,
        }
    }

    /// Run the main event loop
    pub async fn run(&self, mut update_stream: UpdateStream) -> Result<()> {
        info!("Bot started, listening for messages...");

        loop {
            match update_stream.next().await {
                Ok(update) => {
                    if let Err(e) = self.handle_update(update).await {
                        error!("Error handling update: {}", e);
                    }
                }
                Err(e) => {
                    error!("Error getting update: {}", e);
                    // Small delay before retrying
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
        }
    }

    /// Handle an incoming update
    async fn handle_update(&self, update: Update) -> Result<()> {
        match update {
            Update::NewMessage(message) if !message.outgoing() => {
                // The update::Message derefs to the main Message type
                self.handle_message(message.deref()).await?;
            }
            _ => {
                // Ignore other update types
            }
        }
        Ok(())
    }

    /// Handle an incoming message
    async fn handle_message(&self, message: &Message) -> Result<()> {
        let text = message.text();

        // Check if it's a command
        if text.starts_with(&self.config.command_prefix) {
            return self.handle_command(message).await;
        }

        // Check if we should translate this message
        let peer = message.peer();
        let sender = message.sender();

        // Only process group messages
        let is_group = match &peer {
            Ok(Peer::Group(_)) => true,
            Ok(Peer::Channel(_)) => true, // Supergroups are channels in Telegram API
            _ => false,
        };

        if !is_group {
            return Ok(());
        }

        let chat_id = message.peer_id().bare_id();
        let sender_id = match &sender {
            Some(Peer::User(user)) => user.bare_id(),
            _ => return Ok(()),
        };

        // Check if this user should be translated
        let targets = self.targets.read().await;
        if !targets.should_translate(chat_id, sender_id) {
            return Ok(());
        }

        let target_language =
            targets.get_target_language(chat_id, self.translator.default_target_language());
        drop(targets); // Release the lock

        // Translate the message
        if text.is_empty() {
            return Ok(());
        }

        debug!(
            "Translating message from user {} in chat {}",
            sender_id, chat_id
        );

        match self
            .translator
            .translate(text, &target_language, None)
            .await
        {
            Ok(result) => {
                let source_lang = result.detected_language.as_deref().unwrap_or("auto");

                // Don't reply if source and target language are the same
                if source_lang == target_language {
                    debug!("Skipping translation - same language detected");
                    return Ok(());
                }

                let reply_text = self.format_reply(&result.text, source_lang, &target_language);

                message
                    .reply(InputMessage::new().text(&reply_text))
                    .await
                    .context("Failed to send translation reply")?;

                info!(
                    "Translated message from {} ({} -> {})",
                    sender_id, source_lang, target_language
                );
            }
            Err(e) => {
                error!("Translation failed for message '{}': {}", text, e);
                // Optionally notify user of translation failure
                if let Err(reply_err) = message
                    .reply(InputMessage::new().text(&format!("⚠️ Translation failed: {}", e)))
                    .await
                {
                    error!("Failed to send error reply: {}", reply_err);
                }
            }
        }

        Ok(())
    }

    /// Handle a bot command
    async fn handle_command(&self, message: &Message) -> Result<()> {
        let text = message.text();
        let command = self.parse_command(text);

        let sender = message.sender();

        let sender_id = match &sender {
            Some(Peer::User(user)) => user.bare_id(),
            _ => return Ok(()),
        };

        debug!("Received command: {:?} from user {}", command, sender_id);

        match command.name.as_str() {
            "start" | "help" => {
                self.cmd_help(message).await?;
            }
            "translate" | "tr" => {
                self.cmd_translate_user(message, &command.args, sender_id)
                    .await?;
            }
            "untranslate" | "untr" => {
                self.cmd_untranslate_user(message, &command.args, sender_id)
                    .await?;
            }
            "list" => {
                self.cmd_list_users(message).await?;
            }
            "lang" | "language" => {
                self.cmd_set_language(message, &command.args, sender_id)
                    .await?;
            }
            "status" => {
                self.cmd_status(message).await?;
            }
            "enable" => {
                self.cmd_enable(message, sender_id, true).await?;
            }
            "disable" => {
                self.cmd_enable(message, sender_id, false).await?;
            }
            _ => {
                // Unknown command, ignore
            }
        }

        Ok(())
    }

    /// Parse a command from message text
    fn parse_command(&self, text: &str) -> Command {
        let text = text
            .strip_prefix(&self.config.command_prefix)
            .unwrap_or(text);

        let mut parts = text.split_whitespace();
        let cmd_part = parts.next().unwrap_or("");

        // Remove bot username if present (e.g., /command@botname -> command)
        let name = cmd_part.split('@').next().unwrap_or(cmd_part).to_lowercase();
        let args: Vec<String> = parts.map(|s| s.to_string()).collect();

        Command { name, args }
    }

    /// Check if a user is an admin
    fn is_admin(&self, user_id: i64) -> bool {
        self.config.admin_user_ids.contains(&user_id)
    }

    /// Format the translation reply
    fn format_reply(&self, translation: &str, source_lang: &str, target_lang: &str) -> String {
        self.config
            .reply_format
            .replace("{translation}", translation)
            .replace("{source_lang}", source_lang)
            .replace("{target_lang}", target_lang)
    }

    /// Save translation targets to file
    async fn save_targets(&self) -> Result<()> {
        let targets = self.targets.read().await;
        targets.save(&self.targets_file)?;
        Ok(())
    }

    /// Check if the peer is a group
    fn is_group_peer(&self, message: &Message) -> bool {
        let peer_id = message.peer_id();
        matches!(peer_id.kind(), PeerKind::Channel | PeerKind::Chat)
    }

    // ==================== Command Handlers ====================

    /// /help command
    async fn cmd_help(&self, message: &Message) -> Result<()> {
        let help_text = r#"**Telegram Translation Bot**

**Commands:**
- `/translate @user` or `/tr @user` - Translate messages from a user
- `/untranslate @user` or `/untr @user` - Stop translating a user
- `/list` - List users being translated
- `/lang <code>` - Set target language (e.g., `/lang de` for German)
- `/status` - Show current settings
- `/enable` - Enable translation in this chat
- `/disable` - Disable translation in this chat
- `/help` - Show this help message

**Reply Commands:**
- Reply to a message with `/translate` to add that user
- Reply to a message with `/untranslate` to remove that user

**Language Codes:**
en (English), es (Spanish), fr (French), de (German), it (Italian), pt (Portuguese), ru (Russian), zh (Chinese), ja (Japanese), ko (Korean), uk (Ukrainian), pl (Polish), nl (Dutch), tr (Turkish), ar (Arabic), hi (Hindi)

Note: Only group admins can configure translation settings."#;

        message
            .reply(InputMessage::new().text(help_text))
            .await?;
        Ok(())
    }

    /// /translate command
    async fn cmd_translate_user(
        &self,
        message: &Message,
        args: &[String],
        sender_id: i64,
    ) -> Result<()> {
        // Only work in groups
        if !self.is_group_peer(message) {
            message
                .reply(InputMessage::new().text("This command only works in groups."))
                .await?;
            return Ok(());
        }

        // Check permissions (admin only in groups)
        if !self.is_admin(sender_id) {
            message
                .reply(InputMessage::new().text("Only bot admins can use this command."))
                .await?;
            return Ok(());
        }

        // Get target user from args
        let target_user_id = if let Some(user_arg) = args.first() {
            // Parse user ID or username
            self.resolve_user(user_arg).await?
        } else {
            message
                .reply(InputMessage::new().text(
                    "Please specify a user: `/translate @username` or `/translate <user_id>`",
                ))
                .await?;
            return Ok(());
        };

        let target_user_id = match target_user_id {
            Some(id) => id,
            None => {
                message
                    .reply(InputMessage::new().text(
                        "Could not find the specified user. Please use a valid user ID.",
                    ))
                    .await?;
                return Ok(());
            }
        };

        let chat_id = message.peer_id().bare_id();

        // Add user to translation targets
        {
            let mut targets = self.targets.write().await;
            targets.add_user(chat_id, target_user_id);
        }
        self.save_targets().await?;

        message
            .reply(InputMessage::new().text(&format!(
                "Now translating messages from user `{}`.",
                target_user_id
            )))
            .await?;

        info!(
            "Added user {} for translation in chat {}",
            target_user_id, chat_id
        );

        Ok(())
    }

    /// /untranslate command
    async fn cmd_untranslate_user(
        &self,
        message: &Message,
        args: &[String],
        sender_id: i64,
    ) -> Result<()> {
        if !self.is_group_peer(message) {
            message
                .reply(InputMessage::new().text("This command only works in groups."))
                .await?;
            return Ok(());
        }

        if !self.is_admin(sender_id) {
            message
                .reply(InputMessage::new().text("Only bot admins can use this command."))
                .await?;
            return Ok(());
        }

        let target_user_id = if let Some(user_arg) = args.first() {
            self.resolve_user(user_arg).await?
        } else {
            message
                .reply(InputMessage::new().text(
                    "Please specify a user: `/untranslate @username` or `/untranslate <user_id>`",
                ))
                .await?;
            return Ok(());
        };

        let target_user_id = match target_user_id {
            Some(id) => id,
            None => {
                message
                    .reply(InputMessage::new().text("Could not find the specified user."))
                    .await?;
                return Ok(());
            }
        };

        let chat_id = message.peer_id().bare_id();

        let removed = {
            let mut targets = self.targets.write().await;
            targets.remove_user(chat_id, target_user_id)
        };

        if removed {
            self.save_targets().await?;
            message
                .reply(InputMessage::new().text(&format!(
                    "Stopped translating messages from user `{}`.",
                    target_user_id
                )))
                .await?;
            info!(
                "Removed user {} from translation in chat {}",
                target_user_id, chat_id
            );
        } else {
            message
                .reply(InputMessage::new().text("That user was not being translated."))
                .await?;
        }

        Ok(())
    }

    /// /list command
    async fn cmd_list_users(&self, message: &Message) -> Result<()> {
        let chat_id = message.peer_id().bare_id();

        let targets = self.targets.read().await;
        let users = targets.list_users(chat_id);
        let target_lang =
            targets.get_target_language(chat_id, self.translator.default_target_language());

        if users.is_empty() {
            message
                .reply(InputMessage::new().text("No users are being translated in this chat."))
                .await?;
        } else {
            let user_list: Vec<String> = users.iter().map(|id| format!("`{}`", id)).collect();
            let text = format!(
                "**Users being translated:**\n{}\n\n**Target language:** `{}`",
                user_list.join("\n"),
                target_lang
            );
            message.reply(InputMessage::new().text(&text)).await?;
        }

        Ok(())
    }

    /// /lang command
    async fn cmd_set_language(
        &self,
        message: &Message,
        args: &[String],
        sender_id: i64,
    ) -> Result<()> {
        if !self.is_group_peer(message) {
            message
                .reply(InputMessage::new().text("This command only works in groups."))
                .await?;
            return Ok(());
        }

        if !self.is_admin(sender_id) {
            message
                .reply(InputMessage::new().text("Only bot admins can use this command."))
                .await?;
            return Ok(());
        }

        let lang_code = match args.first() {
            Some(code) => {
                let code = code.to_lowercase();
                // Normalize language code (e.g., ua -> uk)
                crate::translator::languages::normalize(&code).to_string()
            }
            None => {
                message
                    .reply(InputMessage::new().text(
                        "Please specify a language code: `/lang <code>`\n\nExamples: en, es, fr, de, it, pt, ru, zh, ja, ko, uk (Ukrainian)",
                    ))
                    .await?;
                return Ok(());
            }
        };

        if !crate::translator::languages::is_valid(&lang_code) {
            message
                .reply(InputMessage::new().text(
                    "Invalid language code. Please use a 2-letter ISO 639-1 code (e.g., en, es, fr, de).",
                ))
                .await?;
            return Ok(());
        }

        let chat_id = message.peer_id().bare_id();

        {
            let mut targets = self.targets.write().await;
            targets.set_target_language(chat_id, lang_code.clone());
        }
        self.save_targets().await?;

        message
            .reply(InputMessage::new().text(&format!("Target language set to `{}`.", lang_code)))
            .await?;

        info!("Set target language to {} in chat {}", lang_code, chat_id);

        Ok(())
    }

    /// /status command
    async fn cmd_status(&self, message: &Message) -> Result<()> {
        let chat_id = message.peer_id().bare_id();

        let targets = self.targets.read().await;
        let group_config = targets.groups.get(&chat_id);

        let (enabled, user_count, target_lang) = match group_config {
            Some(config) => (
                config.enabled,
                config.users.len(),
                config
                    .target_language
                    .clone()
                    .unwrap_or_else(|| self.translator.default_target_language().to_string()),
            ),
            None => (
                true,
                0,
                self.translator.default_target_language().to_string(),
            ),
        };

        let status_text = format!(
            "**Translation Bot Status**\n\n\
            **Chat ID:** `{}`\n\
            **Enabled:** {}\n\
            **Users being translated:** {}\n\
            **Target language:** `{}`",
            chat_id,
            if enabled { "Yes" } else { "No" },
            user_count,
            target_lang
        );

        message
            .reply(InputMessage::new().text(&status_text))
            .await?;

        Ok(())
    }

    /// /enable and /disable commands
    async fn cmd_enable(&self, message: &Message, sender_id: i64, enable: bool) -> Result<()> {
        if !self.is_group_peer(message) {
            message
                .reply(InputMessage::new().text("This command only works in groups."))
                .await?;
            return Ok(());
        }

        if !self.is_admin(sender_id) {
            message
                .reply(InputMessage::new().text("Only bot admins can use this command."))
                .await?;
            return Ok(());
        }

        let chat_id = message.peer_id().bare_id();

        {
            let mut targets = self.targets.write().await;
            targets.set_enabled(chat_id, enable);
        }
        self.save_targets().await?;

        let status = if enable { "enabled" } else { "disabled" };
        message
            .reply(InputMessage::new().text(&format!("Translation has been {}.", status)))
            .await?;

        info!("Translation {} in chat {}", status, chat_id);

        Ok(())
    }

    /// Resolve a user from username or ID string
    async fn resolve_user(&self, user_str: &str) -> Result<Option<i64>> {
        // Remove @ prefix if present
        let user_str = user_str.strip_prefix('@').unwrap_or(user_str);

        // Try to parse as user ID first
        if let Ok(id) = user_str.parse::<i64>() {
            return Ok(Some(id));
        }

        // Resolve username using Telegram API
        debug!("Resolving username: {}", user_str);
        match self.client.resolve_username(user_str).await {
            Ok(Some(peer)) => {
                // Get user ID from the resolved peer
                match peer {
                    Peer::User(user) => {
                        let user_id = user.bare_id();
                        info!("Resolved username @{} to user ID {}", user_str, user_id);
                        Ok(Some(user_id))
                    }
                    _ => {
                        warn!("@{} is not a user (might be a channel/group)", user_str);
                        Ok(None)
                    }
                }
            }
            Ok(None) => {
                warn!("Username @{} not found", user_str);
                Ok(None)
            }
            Err(e) => {
                error!("Failed to resolve username @{}: {}", user_str, e);
                Ok(None)
            }
        }
    }
}
