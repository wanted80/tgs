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
use std::ffi::OsString;

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

    let chunks = match resolve_label(args, environment)? {
        Some(label) => {
            let header = label.header(args.parse_mode);
            split::prepare_labeled_message(message, &header, args.parse_mode, args.max_chunks)?
        }
        None => split::prepare_message(message, args.parse_mode, args.max_chunks)?,
    };
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

#[derive(Debug, Eq, PartialEq)]
struct MessageLabel {
    source: String,
    device: String,
}

impl MessageLabel {
    fn header(&self, parse_mode: split::ParseMode) -> String {
        match parse_mode {
            split::ParseMode::Plain => format!(
                "[{} · {}]",
                escape_plain_label(&self.source),
                escape_plain_label(&self.device)
            ),
            split::ParseMode::Html => format!(
                "[{} · {}]",
                escape_html(&self.source),
                escape_html(&self.device)
            ),
            split::ParseMode::MarkdownV2 => format!(
                "\\[{} · {}\\]",
                escape_markdown_v2(&self.source),
                escape_markdown_v2(&self.device)
            ),
        }
    }
}

fn resolve_label(
    args: &SendArgs,
    environment: &Environment,
) -> Result<Option<MessageLabel>, TgsError> {
    let detected =
        if args.no_label || args.device.is_some() || environment.get("TGS_DEVICE").is_some() {
            None
        } else {
            hostname::get().ok()
        };
    resolve_label_with_hostname(args, environment, detected)
}

fn resolve_label_with_hostname(
    args: &SendArgs,
    environment: &Environment,
    detected_hostname: Option<OsString>,
) -> Result<Option<MessageLabel>, TgsError> {
    if args.no_label {
        if args.label_source.is_some() || args.device.is_some() {
            return Err(TgsError::usage(
                "--no-label cannot be combined with --source or --device",
            ));
        }
        return Ok(None);
    }

    let source = args
        .label_source
        .as_deref()
        .or_else(|| environment.get("TGS_SOURCE"))
        .unwrap_or("tgs");
    validate_identity(source, 64, "source")?;

    let explicit_device = args
        .device
        .as_deref()
        .or_else(|| environment.get("TGS_DEVICE"));
    let device = if let Some(value) = explicit_device {
        validate_identity(value, 255, "device")?;
        value.to_owned()
    } else {
        detected_hostname
            .and_then(|value| value.into_string().ok())
            .filter(|value| validate_identity(value, 255, "device").is_ok())
            .unwrap_or_else(|| "unknown-device".to_owned())
    };

    Ok(Some(MessageLabel {
        source: source.to_owned(),
        device,
    }))
}

pub(crate) fn validate_identity(value: &str, maximum: usize, name: &str) -> Result<(), TgsError> {
    let length = value.chars().count();
    if length == 0 || length > maximum {
        return Err(TgsError::usage(format!(
            "{name} must contain between 1 and {maximum} Unicode characters"
        )));
    }
    if value.trim() != value {
        return Err(TgsError::usage(format!(
            "{name} must not have leading or trailing whitespace"
        )));
    }
    if value.chars().any(char::is_control) {
        return Err(TgsError::usage(format!(
            "{name} must not contain control characters"
        )));
    }
    Ok(())
}

fn escape_plain_label(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]")
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_markdown_v2(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if matches!(
            character,
            '_' | '*'
                | '['
                | ']'
                | '('
                | ')'
                | '~'
                | '`'
                | '>'
                | '#'
                | '+'
                | '-'
                | '='
                | '|'
                | '{'
                | '}'
                | '.'
                | '!'
                | '\\'
        ) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
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

#[cfg(test)]
mod tests {
    use super::*;

    fn environment(values: &[(&str, &str)]) -> Environment {
        Environment::from_values(
            values
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned())),
        )
    }

    #[test]
    fn label_precedence_is_cli_then_environment_then_defaults() {
        let args = SendArgs {
            label_source: Some("cli".to_owned()),
            device: Some("cli-device".to_owned()),
            ..SendArgs::default()
        };
        let env = environment(&[("TGS_SOURCE", "env"), ("TGS_DEVICE", "env-device")]);
        let label = resolve_label_with_hostname(&args, &env, Some(OsString::from("host")))
            .expect("label")
            .expect("enabled");
        assert_eq!(label.source, "cli");
        assert_eq!(label.device, "cli-device");

        let label =
            resolve_label_with_hostname(&SendArgs::default(), &env, Some(OsString::from("host")))
                .expect("label")
                .expect("enabled");
        assert_eq!(label.source, "env");
        assert_eq!(label.device, "env-device");

        let label = resolve_label_with_hostname(
            &SendArgs::default(),
            &Environment::default(),
            Some(OsString::from("host")),
        )
        .expect("label")
        .expect("enabled");
        assert_eq!(label.source, "tgs");
        assert_eq!(label.device, "host");
    }

    #[test]
    fn invalid_hostname_falls_back_without_failing() {
        for hostname in [
            None,
            Some(OsString::from("")),
            Some(OsString::from(" bad ")),
        ] {
            let label = resolve_label_with_hostname(
                &SendArgs::default(),
                &Environment::default(),
                hostname,
            )
            .expect("fallback")
            .expect("enabled");
            assert_eq!(label.device, "unknown-device");
        }
    }

    #[cfg(unix)]
    #[test]
    fn non_unicode_hostname_falls_back_without_failing() {
        use std::os::unix::ffi::OsStringExt;

        let label = resolve_label_with_hostname(
            &SendArgs::default(),
            &Environment::default(),
            Some(OsString::from_vec(vec![0xff])),
        )
        .expect("fallback")
        .expect("enabled");
        assert_eq!(label.device, "unknown-device");
    }

    #[test]
    fn validates_unicode_boundaries_whitespace_and_controls() {
        assert!(validate_identity(&"é".repeat(64), 64, "source").is_ok());
        assert!(validate_identity(&"é".repeat(65), 64, "source").is_err());
        assert!(validate_identity(&"界".repeat(255), 255, "device").is_ok());
        assert!(validate_identity(&"界".repeat(256), 255, "device").is_err());
        for invalid in ["", " leading", "trailing ", "line\nbreak", "nul\0byte"] {
            assert!(validate_identity(invalid, 255, "device").is_err());
        }
    }

    #[test]
    fn no_label_ignores_environment_identity_values() {
        let args = SendArgs {
            no_label: true,
            ..SendArgs::default()
        };
        let env = environment(&[("TGS_SOURCE", " bad "), ("TGS_DEVICE", "bad\nname")]);
        assert_eq!(
            resolve_label_with_hostname(&args, &env, None).expect("disabled"),
            None
        );
    }

    #[test]
    fn escapes_markup_like_label_values_for_each_mode() {
        let label = MessageLabel {
            source: "a<&[b]".to_owned(),
            device: "host_*.example".to_owned(),
        };
        assert_eq!(
            label.header(split::ParseMode::Plain),
            "[a<&\\[b\\] · host_*.example]"
        );
        assert_eq!(
            label.header(split::ParseMode::Html),
            "[a&lt;&amp;[b] · host_*.example]"
        );
        assert_eq!(
            label.header(split::ParseMode::MarkdownV2),
            "\\[a<&\\[b\\] · host\\_\\*\\.example\\]"
        );
    }
}
