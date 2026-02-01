# Telegram Translation Bot

A Telegram bot built with [Grammers](https://github.com/Lonami/grammers) that can be added to groups and configured to automatically translate messages from specific users.

## Features

- **User-specific translation**: Configure which users' messages should be translated in each group
- **Multiple translation providers**: Support for LibreTranslate (free/self-hosted), DeepL, and Google Translate
- **Per-group settings**: Each group can have its own target language and list of translated users
- **Persistent configuration**: Translation settings survive bot restarts
- **Admin controls**: Only group admins can configure translation settings
- **Bot or User mode**: Run as a bot account or as a user account (userbot)

## Requirements

- Rust 1.70 or later
- Telegram API credentials from [my.telegram.org](https://my.telegram.org/apps)
- A bot token from [@BotFather](https://t.me/BotFather) (for bot mode)
- A translation service API key (optional, depending on provider)

## Installation

1. Clone the repository:
   ```bash
   git clone https://github.com/yourusername/tg-translator.git
   cd tg-translator
   ```

2. Copy the example configuration:
   ```bash
   cp .env.example .env
   # or
   cp config.toml.example config.toml
   ```

3. Edit the configuration file with your credentials (see [Configuration](#configuration))

4. Build and run:
   ```bash
   cargo build --release
   ./target/release/tg-translator
   ```

## Configuration

The bot can be configured using either environment variables (`.env` file) or a TOML configuration file (`config.toml`). If both exist, `config.toml` takes precedence.

### Required Settings

| Setting | Environment Variable | Description |
|---------|---------------------|-------------|
| API ID | `TG_API_ID` | Your Telegram API ID from my.telegram.org |
| API Hash | `TG_API_HASH` | Your Telegram API Hash |
| Bot Token | `TG_BOT_TOKEN` | Bot token from @BotFather (bot mode) |

### Optional Settings

| Setting | Environment Variable | Default | Description |
|---------|---------------------|---------|-------------|
| Translation Provider | `TRANSLATION_PROVIDER` | `libretranslate` | `libretranslate`, `deepl`, or `google` |
| Translation API Key | `TRANSLATION_API_KEY` | - | API key for translation service |
| Translation API URL | `TRANSLATION_API_URL` | - | Custom API endpoint URL |
| Target Language | `DEFAULT_TARGET_LANGUAGE` | `en` | Default translation target language |
| Admin User IDs | `ADMIN_USER_IDS` | - | Comma-separated list of admin user IDs |

### Translation Providers

#### LibreTranslate (Default)
- Free and open-source
- Can be self-hosted
- Public instance: `https://libretranslate.com` (may require API key)

```bash
TRANSLATION_PROVIDER=libretranslate
TRANSLATION_API_URL=https://libretranslate.com
# TRANSLATION_API_KEY=your_key_if_required
```

#### DeepL
- High-quality translations
- Requires API key from [DeepL](https://www.deepl.com/pro-api)
- Free tier available

```bash
TRANSLATION_PROVIDER=deepl
TRANSLATION_API_KEY=your_deepl_api_key
```

#### Google Translate
- Uses Lingva Translate as a free frontend
- No API key required

```bash
TRANSLATION_PROVIDER=google
# Optionally specify a different Lingva instance
# TRANSLATION_API_URL=https://lingva.ml/api/v1
```

## Usage

### Adding the Bot to a Group

1. Start a chat with your bot on Telegram
2. Add the bot to your group
3. Make sure the bot has permission to read messages

### Commands

| Command | Description |
|---------|-------------|
| `/help` | Show help message with all commands |
| `/translate @user` or `/tr @user` | Start translating messages from a user |
| `/untranslate @user` or `/untr @user` | Stop translating messages from a user |
| `/list` | List all users being translated in this chat |
| `/lang <code>` | Set the target language (e.g., `/lang de` for German) |
| `/status` | Show current translation settings |
| `/enable` | Enable translation in this chat |
| `/disable` | Disable translation in this chat |

### Language Codes

Common language codes (ISO 639-1):
- `en` - English
- `es` - Spanish
- `fr` - French
- `de` - German
- `it` - Italian
- `pt` - Portuguese
- `ru` - Russian
- `zh` - Chinese
- `ja` - Japanese
- `ko` - Korean
- `uk` - Ukrainian
- `pl` - Polish
- `nl` - Dutch
- `tr` - Turkish
- `ar` - Arabic
- `hi` - Hindi

### Example Workflow

1. Add the bot to your group
2. Set the target language: `/lang en`
3. Add a user to translate: `/translate 123456789` (use the user's numeric ID)
4. Messages from that user will now be automatically translated

## Project Structure

```
tg-translator/
├── Cargo.toml           # Dependencies and project metadata
├── config.toml.example  # Example TOML configuration
├── .env.example         # Example environment configuration
├── README.md            # This file
└── src/
    ├── main.rs          # Entry point and authentication
    ├── bot.rs           # Bot logic and command handlers
    ├── config.rs        # Configuration loading and management
    └── translator.rs    # Translation service abstraction
```

## Data Files

The bot creates the following files during operation:

- `bot.session` - Telegram session data (authentication)
- `translation_targets.json` - Persisted translation configuration per group

These files contain sensitive data and are excluded from version control.

## Running as a User Account (Userbot)

Instead of using a bot token, you can run as a user account:

1. Remove or comment out `TG_BOT_TOKEN` from your configuration
2. Set `TG_PHONE_NUMBER` to your phone number (optional, will prompt if not set)
3. Run the bot and follow the authentication prompts

**Note**: Using a user account allows the bot to read messages without being an admin, but may violate Telegram's Terms of Service. Use at your own risk.

## Development

```bash
# Run with debug logging
RUST_LOG=tg_translator=debug cargo run

# Run tests
cargo test

# Build release binary
cargo build --release
```

## Troubleshooting

### "FloodWait" errors
The bot is being rate-limited by Telegram. Wait for the specified duration before retrying.

### "PeerFlood" errors
Your account may be restricted. Try using a different account or wait a few days.

### Translation not working
- Check your translation provider configuration
- Verify the API key is correct
- Try a different translation provider

### Bot not responding in groups
- Ensure the bot has permission to read messages
- Check that the bot is properly added to the group
- Verify the user ID is correct when using `/translate`

## License

MIT License - see [LICENSE](LICENSE) for details.

## Acknowledgments

- [Grammers](https://github.com/Lonami/grammers) - Rust library for Telegram
- [LibreTranslate](https://libretranslate.com) - Free and open-source translation API
- [Lingva Translate](https://lingva.ml) - Google Translate frontend
