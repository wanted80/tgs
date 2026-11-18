use crate::{cli::OutputMode, error::TgsError};
use serde::Serialize;
use std::io::Write;

/// Versioned successful-delivery report.
#[derive(Debug, Serialize)]
pub struct SuccessReport {
    /// Output schema version.
    pub schema_version: u8,
    /// Success status, currently always `ok`.
    pub status: &'static str,
    /// Resolved local profile name.
    pub profile: String,
    /// Resolved Telegram destination.
    pub destination: String,
    /// Number of delivered chunks.
    pub chunks_sent: usize,
    /// Telegram message IDs for delivered chunks.
    pub message_ids: Vec<i64>,
}

impl SuccessReport {
    /// Writes success output, preserving delivered IDs if writing or flushing fails.
    pub fn write_to(&self, writer: &mut impl Write, mode: OutputMode) -> Result<(), TgsError> {
        let output = match mode {
            OutputMode::Silent => return Ok(()),
            OutputMode::Text => self.text(),
            OutputMode::Json => serde_json::to_string(self).map_err(|_| {
                TgsError::config(
                    "message delivered, but success output could not be encoded; do not resend",
                )
                .with_delivered(self.message_ids.clone())
            })?,
        };
        crate::app::write_line(writer, &output).map_err(|_| {
            TgsError::config(
                "message delivered, but success output could not be written; do not resend",
            )
            .with_delivered(self.message_ids.clone())
        })
    }

    /// Builds a successful report from delivered Telegram message IDs.
    pub fn new(profile: &str, destination: &str, message_ids: Vec<i64>) -> Self {
        Self {
            schema_version: 1,
            status: "ok",
            profile: profile.to_owned(),
            destination: destination.to_owned(),
            chunks_sent: message_ids.len(),
            message_ids,
        }
    }

    /// Formats a human-readable success summary.
    pub fn text(&self) -> String {
        format!(
            "sent {} chunk(s) to {} using profile {} (message IDs: {})",
            self.chunks_sent,
            self.destination,
            self.profile,
            self.message_ids
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_schema_is_versioned_and_contains_no_token_field() {
        let report = SuccessReport::new("ops", "@alerts", vec![42]);
        let value = serde_json::to_value(report).expect("serialize");
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["status"], "ok");
        assert_eq!(value["chunks_sent"], 1);
        assert!(value.get("token").is_none());
    }

    #[test]
    fn failed_output_preserves_delivery_information_including_flush_failures() {
        struct BrokenWriter {
            on_flush: bool,
        }
        impl std::io::Write for BrokenWriter {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                if self.on_flush {
                    Ok(bytes.len())
                } else {
                    Err(std::io::ErrorKind::BrokenPipe.into())
                }
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Err(std::io::ErrorKind::BrokenPipe.into())
            }
        }
        let report = SuccessReport::new("ops", "1", vec![42, 43]);
        for on_flush in [true, false] {
            for mode in [OutputMode::Text, OutputMode::Json] {
                let error = report
                    .write_to(&mut BrokenWriter { on_flush }, mode)
                    .expect_err("output error");
                assert_eq!(error.exit_code(), 3);
                assert_eq!(error.status(), "partial");
                assert_eq!(error.delivered_message_ids(), [42, 43]);
                assert!(error.message().contains("do not resend"));
            }
        }
        assert!(
            report
                .write_to(&mut BrokenWriter { on_flush: false }, OutputMode::Silent)
                .is_ok()
        );
    }
}
