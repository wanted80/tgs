use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::TgsError;
use crate::split::ParseMode;

const MAX_RETRIES: usize = 2;
const MAX_RETRY_AFTER_SECONDS: u64 = 60;

#[derive(Clone, Copy, Debug)]
pub struct SendOptions {
    pub parse_mode: ParseMode,
    pub disable_notification: bool,
    pub disable_web_page_preview: bool,
    pub protect_content: bool,
    pub message_thread_id: Option<i64>,
}

pub trait Sleeper: Clone {
    fn sleep(&self, duration: Duration);
}

#[derive(Clone, Copy, Debug)]
pub struct ThreadSleeper;

impl Sleeper for ThreadSleeper {
    fn sleep(&self, duration: Duration) {
        thread::sleep(duration);
    }
}

pub struct TelegramClient<S = ThreadSleeper> {
    agent: ureq::Agent,
    api_base: String,
    sleeper: S,
}

impl TelegramClient<ThreadSleeper> {
    pub fn new(api_base: &str, timeout_seconds: u64) -> Result<Self, TgsError> {
        Self::with_sleeper(api_base, timeout_seconds, ThreadSleeper)
    }
}

impl<S: Sleeper> TelegramClient<S> {
    pub fn with_sleeper(
        api_base: &str,
        timeout_seconds: u64,
        sleeper: S,
    ) -> Result<Self, TgsError> {
        if timeout_seconds == 0 {
            return Err(TgsError::usage("timeout must be greater than zero"));
        }
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(timeout_seconds)))
            .http_status_as_error(false)
            .build();
        Ok(Self {
            agent: config.into(),
            api_base: api_base.trim_end_matches('/').to_owned(),
            sleeper,
        })
    }

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
            let mut response = self.agent.post(&url).send_json(&request).map_err(|_| {
                TgsError::telegram("Telegram request failed: network or timeout error")
            })?;
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

            if status >= 400 || !envelope.ok {
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
}
