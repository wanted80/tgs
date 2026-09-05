//! Bounded UTF-8 input for messages and secret-bearing configuration.

use std::io::Read;

use crate::error::{ErrorKind, TgsError};

/// Absolute message input ceiling, including when the chunk cap is increased.
pub const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;
/// Maximum configuration file size.
pub const MAX_CONFIG_BYTES: usize = 1024 * 1024;
/// Maximum token input size, including a trailing newline.
pub const MAX_TOKEN_BYTES: usize = 4096;

/// Reads at most `limit + 1` bytes and rejects oversized or non-UTF-8 input.
/// Error messages contain only the caller's static input description.
pub fn read_text(
    reader: impl Read,
    limit: usize,
    kind: ErrorKind,
    description: &'static str,
) -> Result<String, TgsError> {
    let failure = |reason: &str| {
        let message = format!("{description} {reason}");
        match kind {
            ErrorKind::Usage => TgsError::usage(message),
            ErrorKind::Config => TgsError::config(message),
            ErrorKind::Telegram => TgsError::telegram(message),
        }
    };
    let mut bytes = Vec::new();
    reader
        .take(limit.saturating_add(1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| failure("could not be read"))?;
    if bytes.len() > limit {
        return Err(failure("exceeds the input size limit"));
    }
    String::from_utf8(bytes).map_err(|_| failure("must contain valid UTF-8"))
}

/// Conservative byte budget for a message: at most four UTF-8 bytes per scalar.
pub fn message_limit(max_chunks: usize) -> usize {
    max_chunks
        .saturating_mul(crate::split::TELEGRAM_TEXT_LIMIT)
        .saturating_mul(4)
        .min(MAX_MESSAGE_BYTES)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{self, Cursor};

    #[test]
    fn bounded_read_stops_an_endless_source_and_preserves_utf8_boundaries() {
        let mut source = io::repeat(b'a');
        let error = read_text(&mut source, 8, ErrorKind::Usage, "message").expect_err("limit");
        assert_eq!(error.exit_code(), 2);
        assert!(error.message().contains("size limit"));
        assert_eq!(
            read_text("😀".as_bytes(), 4, ErrorKind::Usage, "message").expect("utf8"),
            "😀"
        );
        let mut cursor = Cursor::new(vec![b'a'; 100]);
        assert!(read_text(&mut cursor, 8, ErrorKind::Usage, "message").is_err());
        assert_eq!(cursor.position(), 9);
    }

    #[test]
    fn malformed_text_is_redacted_and_budgets_cannot_overflow() {
        let error = read_text(&b"private\xff"[..], 32, ErrorKind::Config, "configuration")
            .expect_err("utf8");
        assert_eq!(error.exit_code(), 3);
        assert!(!error.message().contains("private"));
        assert_eq!(message_limit(usize::MAX), MAX_MESSAGE_BYTES);
    }
}
