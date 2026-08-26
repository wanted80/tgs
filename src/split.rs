use std::str::FromStr;

use serde::Serialize;

use crate::error::TgsError;

pub const TELEGRAM_TEXT_LIMIT: usize = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum ParseMode {
    Plain,
    Html,
    MarkdownV2,
}

impl ParseMode {
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
            other => Err(TgsError::usage(format!(
                "invalid parse mode `{other}`; expected plain, html, or markdown-v2"
            ))),
        }
    }
}

pub fn prepare_message(
    message: &str,
    parse_mode: ParseMode,
    max_chunks: usize,
) -> Result<Vec<String>, TgsError> {
    if message.is_empty() {
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
        split_plain(message)
    };
    if chunks.len() > max_chunks {
        return Err(TgsError::usage(format!(
            "message requires {} chunks, exceeding --max-chunks {max_chunks}",
            chunks.len()
        )));
    }
    Ok(chunks)
}

fn split_plain(message: &str) -> Vec<String> {
    let characters: Vec<char> = message.chars().collect();
    let mut result = Vec::new();
    let mut start = 0;
    while start < characters.len() {
        let upper = (start + TELEGRAM_TEXT_LIMIT).min(characters.len());
        let mut end = upper;
        if upper < characters.len() {
            end = characters[start..upper]
                .iter()
                .rposition(|character| *character == '\n')
                .map_or(upper, |relative| start + relative + 1);
        }
        result.push(characters[start..end].iter().collect());
        start = end;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_limit_is_one_chunk() {
        let message = "a".repeat(TELEGRAM_TEXT_LIMIT);
        let chunks = prepare_message(&message, ParseMode::Plain, 1).expect("chunks");
        assert_eq!(chunks, [message]);
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
        let message = "x".repeat(TELEGRAM_TEXT_LIMIT + 1);
        assert!(prepare_message(&message, ParseMode::Html, 10).is_err());
        assert!(prepare_message(&message, ParseMode::MarkdownV2, 10).is_err());
    }

    #[test]
    fn rejects_chunk_cap_before_sending() {
        let message = "x".repeat(TELEGRAM_TEXT_LIMIT + 1);
        let error = prepare_message(&message, ParseMode::Plain, 1).expect_err("cap");
        assert_eq!(error.exit_code(), 2);
    }
}
