use std::collections::BTreeMap;
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::bot::{BotChat, BotChatLookup, BotIdentityLookup};
use crate::error::TgsError;
use crate::split::ParseMode;

const MAX_RETRIES: usize = 2;
const MAX_RETRY_AFTER_SECONDS: u64 = 60;

/// Official hosted Telegram Bot API endpoint.
pub const API_BASE: &str = "https://api.telegram.org";

/// Maximum supported per-request timeout, in seconds.
pub const MAX_TIMEOUT_SECONDS: u64 = 300;

/// Validates a timeout before constructing HTTP deadlines.
pub fn validate_timeout(seconds: u64) -> Result<(), TgsError> {
    if !(1..=MAX_TIMEOUT_SECONDS).contains(&seconds) {
        return Err(TgsError::usage(
            "--timeout must be between 1 and 300 seconds",
        ));
    }
    Ok(())
}

/// Telegram delivery flags shared by all chunks.
#[derive(Clone, Copy, Debug)]
pub struct SendOptions {
    /// Telegram formatting mode.
    pub parse_mode: ParseMode,
    /// Whether Telegram notifications are disabled.
    pub disable_notification: bool,
    /// Whether link previews are disabled.
    pub disable_web_page_preview: bool,
    /// Whether forwarding and saving are restricted.
    pub protect_content: bool,
    /// Optional forum topic identifier.
    pub message_thread_id: Option<i64>,
}

/// Injectable retry sleeper used by deterministic transport tests.
pub trait Sleeper: Clone {
    /// Sleeps for the requested retry interval.
    fn sleep(&self, duration: Duration);
}

/// Production sleeper backed by the current thread.
#[derive(Clone, Copy, Debug)]
pub struct ThreadSleeper;

impl Sleeper for ThreadSleeper {
    fn sleep(&self, duration: Duration) {
        thread::sleep(duration);
    }
}

/// Blocking, reusable Telegram Bot API client.
pub struct TelegramClient<S = ThreadSleeper> {
    agent: ureq::Agent,
    api_base: String,
    sleeper: S,
}

/// Public identity fields returned by Telegram's `getMe` method.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BotIdentity {
    /// Stable Telegram bot user ID.
    pub id: i64,
    /// Human-readable bot display name.
    pub display_name: String,
    /// Optional Telegram username, without the leading `@`.
    pub username: Option<String>,
}

impl TelegramClient<ThreadSleeper> {
    /// Creates a production client with a global timeout in seconds.
    pub fn new(api_base: &str, timeout_seconds: u64) -> Result<Self, TgsError> {
        Self::with_sleeper(api_base, timeout_seconds, ThreadSleeper)
    }
}

impl<S: Sleeper> TelegramClient<S> {
    fn inspect<T: serde::de::DeserializeOwned>(
        &self,
        token: &str,
        method: &'static str,
        body: &impl Serialize,
    ) -> Result<T, TgsError> {
        let url = format!("{}/bot{token}/{method}", self.api_base);
        let mut response = self
            .agent
            .post(&url)
            .send_json(body)
            .map_err(|error| request_failure(method, error))?;
        let status = response.status().as_u16();
        let envelope: InspectResponse<T> = response.body_mut().read_json().map_err(|_| {
            TgsError::telegram(format!(
                "Telegram {method} returned an unreadable response (HTTP {status})"
            ))
        })?;
        if !(200..300).contains(&status) || !envelope.ok {
            let description = envelope
                .description
                .as_deref()
                .map(|value| redact(value, token))
                .unwrap_or_else(|| "request rejected".to_owned());
            return Err(TgsError::telegram(format!(
                "Telegram {method} error (HTTP {status}): {description}"
            )));
        }
        envelope.result.ok_or_else(|| {
            TgsError::telegram(format!(
                "Telegram {method} success response omitted its result"
            ))
        })
    }

    /// Looks up a destination without sending a message or changing bot state.
    pub fn inspect_chat(&self, token: &str, chat_id: &str) -> Result<BotChat, TgsError> {
        crate::config::validate_chat_id(chat_id)?;
        let chat: UpdateChat =
            self.inspect(token, "getChat", &serde_json::json!({ "chat_id": chat_id }))?;
        chat.into_bot_chat(token)
    }

    /// Checks for an existing webhook without returning its potentially secret URL.
    pub fn has_webhook(&self, token: &str) -> Result<bool, TgsError> {
        let info: WebhookState = self.inspect(token, "getWebhookInfo", &serde_json::json!({}))?;
        Ok(!info.url.is_empty())
    }

    /// Checks pairing prerequisites without changing webhooks or update filters.
    pub fn check_pairing(&self, token: &str) -> Result<(), TgsError> {
        let info: WebhookState = self.inspect(token, "getWebhookInfo", &serde_json::json!({}))?;
        if !info.url.is_empty() {
            return Err(TgsError::config(
                "this bot already uses a webhook; use `tgs setup --chat-id ID` to keep its integration unchanged",
            ));
        }
        if info.allowed_updates.as_ref().is_some_and(|updates| {
            !updates.is_empty() && !updates.iter().any(|value| value == "message")
        }) {
            return Err(TgsError::config(
                "this bot's existing allowed_updates filter disables message updates needed for pairing; use `tgs setup --chat-id ID` or a dedicated notification bot; its filter has not been changed",
            ));
        }
        Ok(())
    }

    /// Finds a matching Start message without acknowledging or discarding updates.
    /// Only fresh message updates from human users in the requested chat type count.
    pub fn find_pairing(
        &self,
        token: &str,
        challenge: &str,
        username: &str,
        group: bool,
    ) -> Result<Option<BotChat>, TgsError> {
        let updates: Vec<PairingUpdate> =
            self.inspect(token, "getUpdates", &serde_json::json!({}))?;
        let full_queue = updates.len() >= 100;
        let mut candidate: Option<BotChat> = None;
        for update in updates {
            let Some(message) = update.message else {
                continue;
            };
            if message.from.is_none_or(|user| user.is_bot) {
                continue;
            }
            let correct_kind = if group {
                matches!(message.chat.kind.as_str(), "group" | "supergroup")
            } else {
                message.chat.kind == "private"
            };
            if !correct_kind {
                continue;
            }
            let Some(text) = message.text else { continue };
            let mut words = text.split_whitespace();
            let Some(command) = words.next() else {
                continue;
            };
            if command != "/start" && !command.eq_ignore_ascii_case(&format!("/start@{username}")) {
                continue;
            }
            if words.next() != Some(challenge) || words.next().is_some() {
                continue;
            }
            let chat = message.chat.into_bot_chat(token)?;
            if candidate
                .as_ref()
                .is_some_and(|previous| previous.id != chat.id)
            {
                return Err(TgsError::usage(
                    "pairing link was used in multiple chats; restart setup for a new link",
                ));
            }
            candidate = Some(chat);
        }
        if candidate.is_none() && full_queue {
            return Err(TgsError::config(
                "bot has a full pending-update queue; let its existing application process it, or use `tgs setup --chat-id ID`",
            ));
        }
        Ok(candidate)
    }

    /// Creates a client with an injectable retry sleeper.
    pub fn with_sleeper(
        api_base: &str,
        timeout_seconds: u64,
        sleeper: S,
    ) -> Result<Self, TgsError> {
        validate_timeout(timeout_seconds)?;
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(timeout_seconds)))
            .http_status_as_error(false)
            .max_redirects(0)
            .build();
        Ok(Self {
            agent: config.into(),
            api_base: api_base.trim_end_matches('/').to_owned(),
            sleeper,
        })
    }

    /// Sends chunks sequentially and reports any partial delivery.
    pub fn send_chunks(
        &self,
        token: &str,
        chat_id: &str,
        chunks: &[String],
        options: &SendOptions,
    ) -> Result<Vec<i64>, TgsError> {
        let mut delivered = Vec::with_capacity(chunks.len());
        for chunk in chunks {
            match self.send_one(token, chat_id, chunk, options) {
                Ok(message_id) => delivered.push(message_id),
                Err(error) => return Err(error.with_delivered(delivered)),
            }
        }
        Ok(delivered)
    }

    fn fetch_identity(&self, token: &str) -> Result<BotIdentity, TgsError> {
        let url = format!("{}/bot{token}/getMe", self.api_base);
        let mut response = self
            .agent
            .get(&url)
            .call()
            .map_err(|error| request_failure("getMe", error))?;
        let status = response.status().as_u16();
        let envelope: GetMeResponse = response.body_mut().read_json().map_err(|_| {
            TgsError::telegram(format!(
                "Telegram getMe returned an unreadable response (HTTP {status})"
            ))
        })?;
        if !(200..300).contains(&status) || !envelope.ok {
            let code = envelope.error_code.unwrap_or(i64::from(status));
            let description = envelope
                .description
                .as_deref()
                .map(|value| redact(value, token))
                .unwrap_or_else(|| "request rejected".to_owned());
            return Err(TgsError::telegram(format!(
                "Telegram getMe error {code}: {description}"
            )));
        }
        let user = envelope.result.ok_or_else(|| {
            TgsError::telegram("Telegram getMe success response omitted bot identity")
        })?;
        if user.id <= 0
            || !user.is_bot
            || user.first_name.trim().is_empty()
            || user.first_name.chars().any(char::is_control)
            || user
                .last_name
                .as_deref()
                .is_some_and(|value| value.chars().any(char::is_control))
            || user.username.as_deref().is_some_and(|value| {
                value.is_empty()
                    || !value
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            })
        {
            return Err(TgsError::telegram(
                "Telegram getMe returned an invalid bot identity",
            ));
        }
        let first_name = user.first_name.trim();
        Ok(BotIdentity {
            id: user.id,
            display_name: redact(
                &match nonempty_trimmed(user.last_name) {
                    Some(last_name) => format!("{first_name} {last_name}"),
                    None => first_name.to_owned(),
                },
                token,
            ),
            username: user.username,
        })
    }

    fn fetch_recent_chats(&self, token: &str) -> Result<Vec<BotChat>, TgsError> {
        let url = format!("{}/bot{token}/getUpdates", self.api_base);
        let mut response = self
            .agent
            .get(&url)
            .call()
            .map_err(|error| request_failure("getUpdates", error))?;
        let status = response.status().as_u16();
        let envelope: GetUpdatesResponse = response.body_mut().read_json().map_err(|_| {
            TgsError::telegram(format!(
                "Telegram getUpdates returned an unreadable response (HTTP {status})"
            ))
        })?;
        if !(200..300).contains(&status) || !envelope.ok {
            let code = envelope.error_code.unwrap_or(i64::from(status));
            let description = envelope
                .description
                .as_deref()
                .map(|value| redact(value, token))
                .unwrap_or_else(|| "request rejected".to_owned());
            return Err(TgsError::telegram(format!(
                "Telegram getUpdates error {code}: {description}"
            )));
        }

        let updates = envelope.result.ok_or_else(|| {
            TgsError::telegram("Telegram getUpdates success response omitted updates")
        })?;
        let mut chats = BTreeMap::new();
        for update in updates {
            let TelegramUpdate {
                message,
                edited_message,
                channel_post,
                edited_channel_post,
                my_chat_member,
                chat_member,
                chat_join_request,
            } = update;
            for message in [message, edited_message, channel_post, edited_channel_post]
                .into_iter()
                .flatten()
            {
                let chat = message.chat.into_bot_chat(token)?;
                chats.insert(chat.id, chat);
            }
            for membership in [my_chat_member, chat_member].into_iter().flatten() {
                let chat = membership.chat.into_bot_chat(token)?;
                chats.insert(chat.id, chat);
            }
            if let Some(request) = chat_join_request {
                let chat = request.chat.into_bot_chat(token)?;
                chats.insert(chat.id, chat);
            }
        }
        Ok(chats.into_values().collect())
    }

    fn send_one(
        &self,
        token: &str,
        chat_id: &str,
        text: &str,
        options: &SendOptions,
    ) -> Result<i64, TgsError> {
        let url = format!("{}/bot{token}/sendMessage", self.api_base);
        let request = SendMessageRequest {
            chat_id,
            text,
            parse_mode: options.parse_mode.telegram_name(),
            disable_notification: options.disable_notification,
            link_preview_options: options
                .disable_web_page_preview
                .then_some(LinkPreviewOptions { is_disabled: true }),
            protect_content: options.protect_content,
            message_thread_id: options.message_thread_id,
        };

        for attempt in 0..=MAX_RETRIES {
            let mut response = self
                .agent
                .post(&url)
                .send_json(&request)
                .map_err(|error| request_failure("request", error))?;
            let status = response.status().as_u16();
            let envelope: TelegramResponse = response.body_mut().read_json().map_err(|_| {
                TgsError::telegram(format!(
                    "Telegram returned an unreadable response (HTTP {status})"
                ))
            })?;

            if status == 429 {
                let retry_after = envelope
                    .parameters
                    .and_then(|parameters| parameters.retry_after)
                    .ok_or_else(|| {
                        TgsError::telegram("Telegram rate-limited the request without retry_after")
                    })?;
                if retry_after > MAX_RETRY_AFTER_SECONDS {
                    return Err(TgsError::telegram(format!(
                        "Telegram requested a {retry_after}-second retry wait; maximum allowed is 60 seconds"
                    )));
                }
                if attempt == MAX_RETRIES {
                    return Err(TgsError::telegram(
                        "Telegram rate limit persisted after two retries",
                    ));
                }
                self.sleeper.sleep(Duration::from_secs(retry_after));
                continue;
            }

            if !(200..300).contains(&status) || !envelope.ok {
                let code = envelope.error_code.unwrap_or(i64::from(status));
                let description = envelope
                    .description
                    .as_deref()
                    .map(|value| redact(value, token))
                    .unwrap_or_else(|| "request rejected".to_owned());
                return Err(TgsError::telegram(format!(
                    "Telegram API error {code}: {description}"
                )));
            }

            return envelope
                .result
                .map(|message| message.message_id)
                .ok_or_else(|| TgsError::telegram("Telegram success response omitted message_id"));
        }
        Err(TgsError::telegram("Telegram retry state was exhausted"))
    }
}

impl<S: Sleeper> BotIdentityLookup for TelegramClient<S> {
    fn get_me(&self, token: &str) -> Result<BotIdentity, TgsError> {
        self.fetch_identity(token)
    }
}

impl<S: Sleeper> BotChatLookup for TelegramClient<S> {
    fn recent_chats(&self, token: &str) -> Result<Vec<BotChat>, TgsError> {
        self.fetch_recent_chats(token)
    }
}

fn request_failure(method: &'static str, error: ureq::Error) -> TgsError {
    // Never format the underlying error: it may contain the token-bearing URL.
    let reason = if matches!(error, ureq::Error::Timeout(_)) {
        "request timed out"
    } else {
        "network error"
    };
    TgsError::telegram(format!("Telegram {method} failed: {reason}"))
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Serialize)]
struct SendMessageRequest<'a> {
    chat_id: &'a str,
    text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    parse_mode: Option<&'static str>,
    #[serde(skip_serializing_if = "is_false")]
    disable_notification: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    link_preview_options: Option<LinkPreviewOptions>,
    #[serde(skip_serializing_if = "is_false")]
    protect_content: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    message_thread_id: Option<i64>,
}

#[derive(Serialize)]
struct LinkPreviewOptions {
    is_disabled: bool,
}

#[derive(Deserialize)]
struct TelegramResponse {
    ok: bool,
    result: Option<TelegramMessage>,
    description: Option<String>,
    error_code: Option<i64>,
    parameters: Option<ResponseParameters>,
}

#[derive(Deserialize)]
struct TelegramMessage {
    message_id: i64,
}

#[derive(Deserialize)]
struct ResponseParameters {
    retry_after: Option<u64>,
}

#[derive(Deserialize)]
struct GetMeResponse {
    ok: bool,
    result: Option<GetMeUser>,
    description: Option<String>,
    error_code: Option<i64>,
}

#[derive(Deserialize)]
struct GetMeUser {
    id: i64,
    is_bot: bool,
    first_name: String,
    last_name: Option<String>,
    username: Option<String>,
}

#[derive(Deserialize)]
struct GetUpdatesResponse {
    ok: bool,
    result: Option<Vec<TelegramUpdate>>,
    description: Option<String>,
    error_code: Option<i64>,
}

#[derive(Deserialize)]
struct InspectResponse<T> {
    ok: bool,
    result: Option<T>,
    description: Option<String>,
}

#[derive(Deserialize)]
struct WebhookState {
    url: String,
    allowed_updates: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct PairingUpdate {
    message: Option<PairingMessage>,
}

#[derive(Deserialize)]
struct PairingMessage {
    chat: UpdateChat,
    text: Option<String>,
    from: Option<PairingUser>,
}

#[derive(Deserialize)]
struct PairingUser {
    is_bot: bool,
}

#[derive(Deserialize)]
struct TelegramUpdate {
    message: Option<UpdateMessage>,
    edited_message: Option<UpdateMessage>,
    channel_post: Option<UpdateMessage>,
    edited_channel_post: Option<UpdateMessage>,
    my_chat_member: Option<UpdateMembership>,
    chat_member: Option<UpdateMembership>,
    chat_join_request: Option<UpdateMembership>,
}

#[derive(Deserialize)]
struct UpdateMessage {
    chat: UpdateChat,
}

#[derive(Deserialize)]
struct UpdateMembership {
    chat: UpdateChat,
}

#[derive(Deserialize)]
struct UpdateChat {
    id: i64,
    #[serde(rename = "type")]
    kind: String,
    title: Option<String>,
    username: Option<String>,
    first_name: Option<String>,
    last_name: Option<String>,
}

impl UpdateChat {
    fn into_bot_chat(self, token: &str) -> Result<BotChat, TgsError> {
        if self.id == 0
            || self.kind.is_empty()
            || !self
                .kind
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
            || [
                self.title.as_deref(),
                self.username.as_deref(),
                self.first_name.as_deref(),
                self.last_name.as_deref(),
            ]
            .into_iter()
            .flatten()
            .any(|value| value.chars().any(char::is_control))
        {
            return Err(TgsError::telegram(
                "Telegram getUpdates returned invalid chat metadata",
            ));
        }

        let title = nonempty_trimmed(self.title);
        let username = nonempty_trimmed(self.username);
        let first_name = nonempty_trimmed(self.first_name);
        let last_name = nonempty_trimmed(self.last_name);
        let mut name = title
            .or_else(|| match (first_name, last_name) {
                (Some(first), Some(last)) => Some(format!("{first} {last}")),
                (Some(first), None) => Some(first),
                (None, Some(last)) => Some(last),
                (None, None) => None,
            })
            .or_else(|| username.as_ref().map(|value| format!("@{value}")))
            .unwrap_or_else(|| "<unnamed>".to_owned());
        if let Some(value) = username {
            let username_label = format!("@{value}");
            if name != username_label {
                name.push_str(&format!(" ({username_label})"));
            }
        }
        Ok(BotChat {
            id: self.id,
            kind: self.kind,
            name: redact(&name, token),
        })
    }
}

fn nonempty_trimmed(value: Option<String>) -> Option<String> {
    value.and_then(|item| {
        let trimmed = item.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_owned())
    })
}

/// Replaces every occurrence of a token in external text.
pub fn redact(value: &str, token: &str) -> String {
    if token.is_empty() {
        value.to_owned()
    } else {
        value.replace(token, "[REDACTED]")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_every_token_occurrence() {
        let token = "123456:secret";
        let input = format!("bad {token}; URL https://example.test/bot{token}/sendMessage");
        let result = redact(&input, token);
        assert!(!result.contains(token));
        assert_eq!(result.matches("[REDACTED]").count(), 2);
    }

    #[test]
    fn request_omits_unrequested_optional_fields() {
        let request = SendMessageRequest {
            chat_id: "1",
            text: "hello",
            parse_mode: None,
            disable_notification: false,
            link_preview_options: None,
            protect_content: false,
            message_thread_id: None,
        };
        let value = serde_json::to_value(request).expect("serialize");
        assert_eq!(value.as_object().expect("object").len(), 2);
    }

    #[test]
    fn timeouts_are_bounded_before_creating_a_client() {
        for value in [0, 301, u64::MAX] {
            assert_eq!(
                validate_timeout(value)
                    .expect_err("invalid timeout")
                    .exit_code(),
                2
            );
            assert!(TelegramClient::new(API_BASE, value).is_err());
        }
        for value in [1, 300] {
            assert!(validate_timeout(value).is_ok());
        }
    }
}
