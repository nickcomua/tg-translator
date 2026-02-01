//! Telegram Translation Bot
//!
//! A bot that can be added to groups and configured to automatically
//! translate messages from specific users.

mod bot;
mod config;
mod translator;

use anyhow::{Context, Result};
use config::{Config, TranslationTargets};
use grammers_client::{Client, SignInError, UpdatesConfiguration};
use grammers_mtsender::SenderPool;
use grammers_session::storages::SqliteSession;
use std::io::{self, BufRead, Write};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

const TARGETS_FILE: &str = "translation_targets.json";

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "tg_translator=info,grammers=warn".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("Starting Telegram Translation Bot");

    // Load configuration
    let config = Config::load().context("Failed to load configuration")?;
    info!("Configuration loaded successfully");

    // Load translation targets
    let targets =
        TranslationTargets::load(TARGETS_FILE).context("Failed to load translation targets")?;
    let shared_targets = Arc::new(RwLock::new(targets));
    info!("Translation targets loaded");

    // Create session storage
    let session = Arc::new(
        SqliteSession::open(&config.telegram.session_file)
            .context("Failed to open or create session file")?,
    );

    // Create sender pool
    let pool = SenderPool::new(Arc::clone(&session), config.telegram.api_id);

    // Create client (must be done before moving the pool)
    let client = Client::new(&pool);
    let updates_rx = pool.updates;
    let pool_handle = pool.handle.clone();

    // Spawn the pool runner in the background
    let runner_handle = tokio::spawn(async move {
        pool.runner.run().await;
    });

    info!("Connecting to Telegram...");

    // Authenticate
    if !client.is_authorized().await? {
        info!("Not authorized, starting authentication...");
        authenticate(&client, &config).await?;
    }

    info!("Authenticated successfully");

    // Print bot info
    let me = client.get_me().await?;
    info!(
        "Logged in as: {} (ID: {})",
        me.first_name().unwrap_or("Unknown"),
        me.bare_id()
    );

    // Create translator
    let translator = translator::Translator::new(config.translation.clone());

    // Create update stream
    let updates_config = UpdatesConfiguration::default();
    let update_stream = client.stream_updates(updates_rx, updates_config);

    // Create and run the bot
    let bot = bot::Bot::new(
        client.clone(),
        translator,
        shared_targets,
        config.bot.clone(),
        TARGETS_FILE.to_string(),
        config.telegram.api_hash.clone(),
    );

    info!("Bot is ready! Listening for messages...");
    println!("\n===========================================");
    println!("  Telegram Translation Bot is running!");
    println!("  Press Ctrl+C to stop");
    println!("===========================================\n");

    // Run the bot with the update stream
    let bot_result = bot.run(update_stream).await;

    // Signal the pool to quit
    pool_handle.quit();

    // Wait for the pool runner to finish
    let _ = runner_handle.await;

    bot_result
}

/// Handle authentication (bot token or user account)
async fn authenticate(client: &Client, config: &Config) -> Result<()> {
    let api_hash = &config.telegram.api_hash;

    // Try bot token authentication first
    if let Some(ref token) = config.telegram.bot_token {
        info!("Authenticating with bot token...");
        client
            .bot_sign_in(token, api_hash)
            .await
            .context("Failed to sign in with bot token")?;
        return Ok(());
    }

    // Fall back to user authentication
    let phone = if let Some(ref phone) = config.telegram.phone_number {
        phone.clone()
    } else {
        // Prompt for phone number
        print!("Enter your phone number (international format, e.g., +1234567890): ");
        io::stdout().flush()?;
        let stdin = io::stdin();
        let mut phone = String::new();
        stdin.lock().read_line(&mut phone)?;
        phone.trim().to_string()
    };

    info!("Requesting login code for {}...", phone);
    let token = client
        .request_login_code(&phone, api_hash)
        .await
        .context("Failed to request login code")?;

    // Prompt for the code
    print!("Enter the code you received: ");
    io::stdout().flush()?;
    let stdin = io::stdin();
    let mut code = String::new();
    stdin.lock().read_line(&mut code)?;
    let code = code.trim();

    // Try to sign in
    match client.sign_in(&token, code).await {
        Ok(_) => {
            info!("Signed in successfully!");
        }
        Err(SignInError::PasswordRequired(password_token)) => {
            // 2FA is enabled
            warn!("Two-factor authentication is enabled");
            print!("Enter your 2FA password: ");
            io::stdout().flush()?;

            let mut password = String::new();
            stdin.lock().read_line(&mut password)?;
            let password = password.trim();

            client
                .check_password(password_token, password)
                .await
                .context("Failed to verify 2FA password")?;

            info!("Signed in with 2FA!");
        }
        Err(e) => {
            return Err(e).context("Failed to sign in");
        }
    }

    Ok(())
}
