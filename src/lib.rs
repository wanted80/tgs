#![doc = "Library behavior for the `tgs` Telegram notification CLI."]
#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::expect_used, clippy::panic))]

/// Friendly local bot-profile operations.
pub mod bot;
/// Command-line argument models and parsing.
pub mod cli;
/// Versioned profile configuration and secure persistence.
pub mod config;
/// Stable error categories and failure reports.
pub mod error;
/// Successful delivery output models.
pub mod output;
/// Telegram-aware message validation and splitting.
pub mod split;
/// Blocking Telegram Bot API transport.
pub mod telegram;

use std::collections::BTreeMap;

use cli::SendArgs;
use config::{Config, resolve_config_path};
use error::TgsError;
use output::SuccessReport;
use telegram::{SendOptions, TelegramClient};

/// Captured environment values used for deterministic precedence resolution.
#[derive(Clone, Debug, Default)]
pub struct Environment {
    values: BTreeMap<String, String>,
}

impl Environment {
    /// Captures the current process environment.
    pub fn capture() -> Self {
        Self {
            values: std::env::vars().collect(),
        }
    }

    /// Builds an environment from explicit values for deterministic resolution.
    pub fn from_values(values: impl IntoIterator<Item = (String, String)>) -> Self {
        Self {
            values: values.into_iter().collect(),
        }
    }

    /// Returns a captured value without consulting the live process environment.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }
}

/// Resolves configuration and sends a validated message.
pub fn send_message(
    args: &SendArgs,
    message: &str,
    environment: &Environment,
) -> Result<SuccessReport, TgsError> {
    send_message_with_base(args, message, environment, "https://api.telegram.org")
}

/// Endpoint-injectable send path used by offline integration tests.
pub fn send_message_with_base(
    args: &SendArgs,
    message: &str,
    environment: &Environment,
    api_base: &str,
) -> Result<SuccessReport, TgsError> {
    if message.is_empty() {
        return Err(TgsError::usage("message must not be empty"));
    }

    let config_path = resolve_config_path(args.config.as_deref(), environment)?;
    let config = Config::load_optional(&config_path)?;
    let profile_name = args
        .profile
        .as_deref()
        .or_else(|| environment.get("TGS_PROFILE"))
        .or(config.default_profile.as_deref())
        .ok_or_else(|| TgsError::config("no profile selected; run `tgs config init`"))?;
    config::validate_profile_name(profile_name)?;
    let profile = config.profiles.get(profile_name);

    let token = environment
        .get("TGS_TOKEN")
        .or_else(|| profile.map(|item| item.token.as_str()))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| TgsError::config(format!("profile `{profile_name}` has no token")))?;
    validate_token(token)?;
    let destination = args
        .chat_id
        .as_deref()
        .or_else(|| environment.get("TGS_CHAT_ID"))
        .or_else(|| profile.and_then(|item| item.chat_id.as_deref()))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| TgsError::config(format!("profile `{profile_name}` has no chat ID")))?;
    validate_chat_id(destination)?;

    let chunks = split::prepare_message(message, args.parse_mode, args.max_chunks)?;
    let options = SendOptions {
        parse_mode: args.parse_mode,
        disable_notification: args.silent,
        disable_web_page_preview: args.no_preview,
        protect_content: args.protect,
        message_thread_id: args.topic,
    };
    let client = TelegramClient::new(api_base, args.timeout)?;
    let message_ids = client.send_chunks(token, destination, &chunks, &options)?;

    Ok(SuccessReport::new(profile_name, destination, message_ids))
}

fn validate_token(token: &str) -> Result<(), TgsError> {
    if token.chars().any(char::is_control) || token.chars().any(char::is_whitespace) {
        return Err(TgsError::config(
            "Telegram token contains invalid whitespace or control characters",
        ));
    }
    Ok(())
}

fn validate_chat_id(chat_id: &str) -> Result<(), TgsError> {
    if chat_id.chars().any(char::is_control) || chat_id.chars().any(char::is_whitespace) {
        return Err(TgsError::config(
            "chat ID contains invalid whitespace or control characters",
        ));
    }
    Ok(())
}
