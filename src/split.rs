use std::str::FromStr;

use serde::Serialize;

use crate::error::TgsError;

/// Maximum Telegram text length in Unicode scalar values.
pub const TELEGRAM_TEXT_LIMIT: usize = 4096;

/// Supported Telegram formatting modes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum ParseMode {
    /// Unformatted text that may be split safely.
    Plain,
    /// Telegram HTML formatting.
    Html,
    /// Telegram MarkdownV2 formatting.
    MarkdownV2,
}

impl ParseMode {
    /// Returns the Telegram API name, or none for plain text.
    pub const fn telegram_name(self) -> Option<&'static str> {
        match self {
            Self::Plain => None,
            Self::Html => Some("HTML"),
            Self::MarkdownV2 => Some("MarkdownV2"),
        }
    }
}

impl FromStr for ParseMode {
    type Err = TgsError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "plain" => Ok(Self::Plain),
            "html" => Ok(Self::Html),
            "markdown-v2" => Ok(Self::MarkdownV2),
            _ => Err(TgsError::usage(
                "invalid parse mode; expected plain, html, or markdown-v2",
            )),
        }
    }
}

/// Validates and splits a message before any network request.
pub fn prepare_message(
    message: &str,
    parse_mode: ParseMode,
    max_chunks: usize,
) -> Result<Vec<String>, TgsError> {
    validate_size(message, max_chunks)?;
    if message.trim().is_empty() {
        return Err(TgsError::usage("message must not be empty"));
    }
    let character_count = message.chars().count();
    if parse_mode != ParseMode::Plain && character_count > TELEGRAM_TEXT_LIMIT {
        return Err(TgsError::usage(
            "formatted messages cannot exceed 4096 characters; arbitrary splitting could invalidate formatting",
        ));
    }
    let chunks = if character_count <= TELEGRAM_TEXT_LIMIT {
        vec![message.to_owned()]
    } else {
        split_plain_at(message, TELEGRAM_TEXT_LIMIT, max_chunks)?
    };
    if chunks.len() > max_chunks {
        return Err(TgsError::usage(format!(
            "message requires {} chunks, exceeding --max-chunks {max_chunks}",
            chunks.len()
        )));
    }
    Ok(chunks)
}

/// Prepares messages with a generated header while preserving Telegram limits.
pub(crate) fn prepare_labeled_message(
    message: &str,
    header: &str,
    parse_mode: ParseMode,
    max_chunks: usize,
) -> Result<Vec<String>, TgsError> {
    validate_size(message, max_chunks)?;
    if message.trim().is_empty() {
        return Err(TgsError::usage("message must not be empty"));
    }
    let prefix = format!("{header}\n");
    let prefix_length = prefix.chars().count();
    let capacity = TELEGRAM_TEXT_LIMIT
        .checked_sub(prefix_length)
        .ok_or_else(|| {
            TgsError::usage("generated message label exceeds Telegram's 4096-character limit")
        })?;
    if capacity == 0 {
        return Err(TgsError::usage(
            "generated message label leaves no room for message text",
        ));
    }

    let message_length = message.chars().count();
    if parse_mode != ParseMode::Plain && message_length > capacity {
        return Err(TgsError::usage(
            "formatted message and label cannot exceed 4096 characters; arbitrary splitting could invalidate formatting",
        ));
    }
    let bodies = if message_length <= capacity {
        vec![message.to_owned()]
    } else {
        split_plain_at(message, capacity, max_chunks)?
    };
    if bodies.len() > max_chunks {
        return Err(TgsError::usage(format!(
            "message requires {} chunks, exceeding --max-chunks {max_chunks}",
            bodies.len()
        )));
    }
    Ok(bodies
        .into_iter()
        .map(|body| format!("{prefix}{body}"))
        .collect())
}

fn validate_size(message: &str, max_chunks: usize) -> Result<(), TgsError> {
    if max_chunks == 0 {
        return Err(TgsError::usage("--max-chunks must be greater than zero"));
    }
    if message.len() > crate::input::message_limit(max_chunks) {
        return Err(TgsError::usage(
            "message exceeds the input size limit for --max-chunks (absolute maximum 16 MiB)",
        ));
    }
    Ok(())
}

fn split_plain_at(
    mut message: &str,
    limit: usize,
    max_chunks: usize,
) -> Result<Vec<String>, TgsError> {
    let mut result = Vec::new();
    while !message.is_empty() {
        if result.len() == max_chunks {
            return Err(TgsError::usage(format!(
                "message exceeds --max-chunks {max_chunks}"
            )));
        }
        let upper = message
            .char_indices()
            .nth(limit)
            .map_or(message.len(), |(index, _)| index);
        let mut end = upper;
        if upper < message.len() {
            end = message[..upper]
                .rfind('\n')
                .map_or(upper, |index| index + 1);
            // A preferred newline must not create a blank chunk or strand a
            // run of whitespace at the start of the next chunk.
            if message[..end].trim().is_empty()
                || message[end..].chars().take(limit).all(char::is_whitespace)
            {
                end = upper;
            }
            // Keep a visible character with a short whitespace-only tail. This
            // preserves the input exactly (including a file's final newline).
            if message[end..].chars().take(limit).all(char::is_whitespace) {
                if let Some((last_text, _)) = message[..end]
                    .char_indices()
                    .rfind(|(_, character)| !character.is_whitespace())
                {
                    if message[last_text..].chars().take(limit + 1).count() <= limit {
                        end = last_text;
                    }
                }
            }
        }
        if message[..end].trim().is_empty() {
            return Err(TgsError::usage(
                "message would contain a whitespace-only chunk; remove excess whitespace before sending",
            ));
        }
        result.push(message[..end].to_owned());
        message = &message[end..];
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_modes_map_to_the_exact_telegram_api_values() {
        assert_eq!(ParseMode::Plain.telegram_name(), None);
        assert_eq!(ParseMode::Html.telegram_name(), Some("HTML"));
        assert_eq!(ParseMode::MarkdownV2.telegram_name(), Some("MarkdownV2"));
    }

    #[test]
    fn exact_limit_is_one_chunk() {
        let message = "a".repeat(TELEGRAM_TEXT_LIMIT);
        let chunks = prepare_message(&message, ParseMode::Plain, 1).expect("chunks");
        assert_eq!(chunks, [message]);
    }

    #[test]
    fn whitespace_boundaries_keep_text_in_each_chunk_and_preserve_input() {
        for message in [
            format!("{}\n", "a".repeat(TELEGRAM_TEXT_LIMIT)),
            format!("{}\r\n", "😀".repeat(TELEGRAM_TEXT_LIMIT)),
            format!("\n{}", "é".repeat(TELEGRAM_TEXT_LIMIT)),
            format!("a\n{}b", " ".repeat(TELEGRAM_TEXT_LIMIT)),
        ] {
            let chunks = prepare_message(&message, ParseMode::Plain, 3).expect("chunks");
            assert_eq!(chunks.concat(), message);
            assert!(chunks.iter().all(|chunk| !chunk.trim().is_empty()));
            assert!(
                chunks
                    .iter()
                    .all(|chunk| chunk.chars().count() <= TELEGRAM_TEXT_LIMIT)
            );
        }
    }

    #[test]
    fn unsendable_whitespace_is_rejected_before_delivery_with_or_without_labels() {
        for message in [
            " \r\n\t\u{2003}".to_owned(),
            format!("a{}", " ".repeat(TELEGRAM_TEXT_LIMIT * 2)),
        ] {
            assert_eq!(
                prepare_message(&message, ParseMode::Plain, 10)
                    .expect_err("blank chunk")
                    .exit_code(),
                2
            );
            assert_eq!(
                prepare_labeled_message(&message, "[tgs · host]", ParseMode::Plain, 10)
                    .expect_err("blank body")
                    .exit_code(),
                2
            );
        }
    }

    #[test]
    fn labeled_newline_tails_do_not_create_label_only_notifications() {
        let header = "[tgs · host]";
        let prefix = format!("{header}\n");
        let message = format!(
            "{}\n",
            "x".repeat(TELEGRAM_TEXT_LIMIT - prefix.chars().count())
        );
        let chunks =
            prepare_labeled_message(&message, header, ParseMode::Plain, 2).expect("chunks");
        let bodies: Vec<_> = chunks
            .iter()
            .map(|chunk| chunk.strip_prefix(&prefix).expect("label"))
            .collect();
        assert_eq!(bodies.concat(), message);
        assert!(bodies.iter().all(|body| !body.trim().is_empty()));
    }

    #[test]
    fn small_exhaustive_splits_preserve_unicode_and_never_emit_blank_chunks() {
        let alphabet = ['a', ' ', '\n', '😀'];
        for mut code in 0..4096 {
            let mut message = String::new();
            for _ in 0..6 {
                message.push(alphabet[code % alphabet.len()]);
                code /= alphabet.len();
            }
            for limit in 1..=5 {
                if let Ok(chunks) = split_plain_at(&message, limit, 6) {
                    assert_eq!(chunks.concat(), message);
                    assert!(chunks.len() <= 6);
                    assert!(chunks.iter().all(|chunk| {
                        !chunk.trim().is_empty() && chunk.chars().count() <= limit
                    }));
                }
            }
        }
    }

    #[test]
    fn splits_on_unicode_scalar_boundaries() {
        let message = "😀".repeat(TELEGRAM_TEXT_LIMIT + 1);
        let chunks = prepare_message(&message, ParseMode::Plain, 2).expect("chunks");
        assert_eq!(chunks[0].chars().count(), TELEGRAM_TEXT_LIMIT);
        assert_eq!(chunks[1], "😀");
        assert_eq!(chunks.concat(), message);
    }

    #[test]
    fn prefers_the_last_newline_within_limit() {
        let message = format!("{}\n{}", "a".repeat(4000), "b".repeat(200));
        let chunks = prepare_message(&message, ParseMode::Plain, 2).expect("chunks");
        assert_eq!(chunks[0].chars().count(), 4001);
        assert!(chunks[0].ends_with('\n'));
        assert_eq!(chunks.concat(), message);
    }

    #[test]
    fn long_line_is_split_at_limit() {
        let message = "é".repeat(TELEGRAM_TEXT_LIMIT * 2 + 1);
        let chunks = prepare_message(&message, ParseMode::Plain, 3).expect("chunks");
        assert_eq!(
            chunks
                .iter()
                .map(|chunk| chunk.chars().count())
                .collect::<Vec<_>>(),
            [4096, 4096, 1]
        );
    }

    #[test]
    fn rejects_oversized_formatted_messages() {
        let exact = "x".repeat(TELEGRAM_TEXT_LIMIT);
        for mode in [ParseMode::Html, ParseMode::MarkdownV2] {
            let chunks = prepare_message(&exact, mode, 1).expect("exact formatted boundary");
            assert_eq!(chunks.len(), 1);
            assert_eq!(chunks[0], exact);
        }

        let message = "x".repeat(TELEGRAM_TEXT_LIMIT + 1);
        assert!(prepare_message(&message, ParseMode::Html, 10).is_err());
        assert!(prepare_message(&message, ParseMode::MarkdownV2, 10).is_err());
    }

    #[test]
    fn does_not_split_a_final_fitting_chunk_at_its_last_newline() {
        let message = format!("{}b\nc", "a".repeat(TELEGRAM_TEXT_LIMIT));
        let chunks = prepare_message(&message, ParseMode::Plain, 2).expect("chunks");
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0], "a".repeat(TELEGRAM_TEXT_LIMIT));
        assert_eq!(chunks[1], "b\nc");
        assert_eq!(chunks.concat(), message);
    }

    #[test]
    fn rejects_chunk_cap_before_sending() {
        let message = "x".repeat(TELEGRAM_TEXT_LIMIT + 1);
        let error = prepare_message(&message, ParseMode::Plain, 1).expect_err("cap");
        assert_eq!(error.exit_code(), 2);
    }

    #[test]
    fn labeled_plain_text_reserves_header_space_on_every_chunk() {
        let header = "[ci · runner]";
        let capacity = TELEGRAM_TEXT_LIMIT - header.chars().count() - 1;
        let message = "x".repeat(capacity + 1);
        let chunks =
            prepare_labeled_message(&message, header, ParseMode::Plain, 2).expect("labeled chunks");
        assert_eq!(chunks.len(), 2);
        assert!(
            chunks
                .iter()
                .all(|chunk| chunk.starts_with("[ci · runner]\n"))
        );
        assert_eq!(chunks[0].chars().count(), TELEGRAM_TEXT_LIMIT);
        assert_eq!(chunks[1], "[ci · runner]\nx");
    }

    #[test]
    fn labeled_splitting_prefers_newlines_with_reduced_capacity() {
        let header = "[tgs · host]";
        let capacity = TELEGRAM_TEXT_LIMIT - header.chars().count() - 1;
        let message = format!("{}\n{}", "a".repeat(capacity - 10), "b".repeat(20));
        let chunks =
            prepare_labeled_message(&message, header, ParseMode::Plain, 2).expect("labeled chunks");
        assert!(chunks[0].ends_with('\n'));
        assert_eq!(chunks[0].chars().count(), TELEGRAM_TEXT_LIMIT - 9);
        let bodies = chunks
            .iter()
            .map(|chunk| chunk.strip_prefix(&format!("{header}\n")).expect("header"))
            .collect::<String>();
        assert_eq!(bodies, message);
    }

    #[test]
    fn labeled_boundaries_and_chunk_cap_are_checked() {
        let header = "[tgs · host]";
        let capacity = TELEGRAM_TEXT_LIMIT - header.chars().count() - 1;
        let exact = prepare_labeled_message(&"é".repeat(capacity), header, ParseMode::Plain, 1)
            .expect("exact boundary");
        assert_eq!(exact[0].chars().count(), TELEGRAM_TEXT_LIMIT);

        let error = prepare_labeled_message(&"x".repeat(capacity + 1), header, ParseMode::Plain, 1)
            .expect_err("chunk cap");
        assert!(error.message().contains("--max-chunks"));
    }

    #[test]
    fn formatted_labeled_messages_are_unsplittable() {
        let header = "[source · device]";
        let capacity = TELEGRAM_TEXT_LIMIT - header.chars().count() - 1;
        for mode in [ParseMode::Html, ParseMode::MarkdownV2] {
            assert!(prepare_labeled_message(&"x".repeat(capacity), header, mode, 1).is_ok());
            assert!(prepare_labeled_message(&"x".repeat(capacity + 1), header, mode, 10).is_err());
        }
    }
}
