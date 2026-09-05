use std::fmt;

use serde::Serialize;

/// Stable top-level failure category used to select a process exit code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorKind {
    /// Invalid arguments, input, or locally validated values.
    Usage,
    /// Configuration or filesystem failure.
    Config,
    /// Telegram API, network, retry, or partial-delivery failure.
    Telegram,
}

impl ErrorKind {
    /// Returns the stable process exit code for this category.
    pub const fn exit_code(self) -> u8 {
        match self {
            Self::Usage => 2,
            Self::Config => 3,
            Self::Telegram => 4,
        }
    }
}

/// Actionable application error with optional partial-delivery metadata.
#[derive(Debug)]
pub struct TgsError {
    kind: ErrorKind,
    message: String,
    delivered_message_ids: Vec<i64>,
}

impl TgsError {
    /// Writes a JSON report or a readable terminal error, including known deliveries.
    pub fn write_to(&self, writer: &mut impl std::io::Write, json: bool) -> std::io::Result<()> {
        if json {
            serde_json::to_writer(&mut *writer, &FailureReport::from(self))
                .map_err(std::io::Error::other)?;
            writer.write_all(b"\n")?;
        } else {
            // External descriptions must not be able to inject terminal controls.
            let message: String = self
                .message
                .chars()
                .flat_map(|c| {
                    if c.is_control() {
                        c.escape_default().collect::<Vec<_>>()
                    } else {
                        vec![c]
                    }
                })
                .collect();
            writeln!(writer, "error: {message}")?;
            if !self.delivered_message_ids.is_empty() {
                writeln!(
                    writer,
                    "delivered message IDs: {:?}; do not resend the whole message",
                    self.delivered_message_ids
                )?;
            }
        }
        writer.flush()
    }

    /// Creates a usage or validation error.
    pub fn usage(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Usage, message)
    }

    /// Creates a configuration or filesystem error.
    pub fn config(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Config, message)
    }

    /// Creates a Telegram or network error.
    pub fn telegram(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Telegram, message)
    }

    fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            delivered_message_ids: Vec::new(),
        }
    }

    /// Attaches message IDs delivered before a later failure.
    pub fn with_delivered(mut self, ids: Vec<i64>) -> Self {
        self.delivered_message_ids = ids;
        self
    }

    /// Returns the stable error category.
    pub const fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// Returns the stable process exit code.
    pub const fn exit_code(&self) -> u8 {
        self.kind.exit_code()
    }

    /// Returns the redacted, user-facing error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns message IDs delivered before a partial failure.
    pub fn delivered_message_ids(&self) -> &[i64] {
        &self.delivered_message_ids
    }

    /// Returns `partial` after any delivery, otherwise `error`.
    pub fn status(&self) -> &'static str {
        if self.delivered_message_ids.is_empty() {
            "error"
        } else {
            "partial"
        }
    }
}

impl fmt::Display for TgsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for TgsError {}

/// Versioned structured failure output.
#[derive(Serialize)]
pub struct FailureReport<'a> {
    /// Output schema version.
    pub schema_version: u8,
    /// Either `error` or `partial`.
    pub status: &'static str,
    /// Redacted actionable message.
    pub error: &'a str,
    /// Number of chunks delivered before failure.
    pub chunks_sent: usize,
    /// Telegram message IDs delivered before failure.
    pub message_ids: &'a [i64],
}

impl<'a> From<&'a TgsError> for FailureReport<'a> {
    fn from(error: &'a TgsError) -> Self {
        Self {
            schema_version: 1,
            status: error.status(),
            error: error.message(),
            chunks_sent: error.delivered_message_ids().len(),
            message_ids: error.delivered_message_ids(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_are_stable() {
        assert_eq!(TgsError::usage("usage").exit_code(), 2);
        assert_eq!(TgsError::config("config").exit_code(), 3);
        assert_eq!(TgsError::telegram("telegram").exit_code(), 4);
    }

    #[test]
    fn delivered_ids_change_failure_status_to_partial() {
        let error = TgsError::telegram("later chunk failed").with_delivered(vec![7, 8]);
        let report = FailureReport::from(&error);
        assert_eq!(report.status, "partial");
        assert_eq!(report.chunks_sent, 2);
        assert_eq!(report.message_ids, [7, 8]);
    }

    #[test]
    fn terminal_and_json_errors_preserve_delivery_and_escape_controls() {
        let error = TgsError::telegram("failure\u{1b}[2J").with_delivered(vec![42]);
        let mut text = Vec::new();
        error.write_to(&mut text, false).expect("text");
        let text = String::from_utf8(text).expect("utf8");
        assert!(!text.contains('\u{1b}'));
        assert!(text.contains("42"));
        assert!(text.contains("do not resend"));
        let mut json = Vec::new();
        error.write_to(&mut json, true).expect("json");
        let json: serde_json::Value = serde_json::from_slice(&json).expect("schema");
        assert_eq!(json["status"], "partial");
        assert_eq!(json["message_ids"], serde_json::json!([42]));
    }
}
