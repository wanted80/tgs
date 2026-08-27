use serde::Serialize;

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
}
