use chrono::DateTime;
use chrono::FixedOffset;
use chrono::Local;
use chrono::SecondsFormat;
use codex_app_server_protocol::AdditionalContextEntry;
use codex_app_server_protocol::AdditionalContextKind;
use std::collections::HashMap;

const PROMPT_TIMESTAMP_CONTEXT_KEY: &str = "prompt_timestamp";

pub(crate) struct PromptSubmissionTimestamp {
    pub(crate) context_value: String,
    pub(crate) created_at_ms: i64,
}

pub(crate) fn current_prompt_submission_timestamp() -> PromptSubmissionTimestamp {
    prompt_submission_timestamp(Local::now().fixed_offset())
}

pub(crate) fn prompt_timestamp_additional_context(
    prompt_submitted_at: &str,
) -> HashMap<String, AdditionalContextEntry> {
    HashMap::from([(
        PROMPT_TIMESTAMP_CONTEXT_KEY.to_string(),
        AdditionalContextEntry {
            value: prompt_submitted_at.to_string(),
            kind: AdditionalContextKind::Application,
        },
    )])
}

fn format_prompt_submitted_at(prompt_submitted_at: DateTime<FixedOffset>) -> String {
    prompt_submitted_at.to_rfc3339_opts(SecondsFormat::Nanos, false)
}

fn prompt_submission_timestamp(
    prompt_submitted_at: DateTime<FixedOffset>,
) -> PromptSubmissionTimestamp {
    PromptSubmissionTimestamp {
        context_value: format_prompt_submitted_at(prompt_submitted_at),
        created_at_ms: prompt_submitted_at.timestamp_millis(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_prompt_timestamp_with_local_offset_and_nanoseconds() {
        let prompt_submitted_at =
            DateTime::parse_from_rfc3339("2026-08-28T14:22:31.123456789-06:00")
                .expect("timestamp should parse");
        let prompt_submission = prompt_submission_timestamp(prompt_submitted_at);

        assert_eq!(
            prompt_submission.context_value,
            "2026-08-28T14:22:31.123456789-06:00"
        );
        assert_eq!(prompt_submission.created_at_ms, 1_787_948_551_123);
    }

    #[test]
    fn builds_application_context_without_changing_prompt_text() {
        let prompt_submitted_at = "2026-08-28T14:22:31.123456789-06:00";

        assert_eq!(
            prompt_timestamp_additional_context(prompt_submitted_at),
            HashMap::from([(
                "prompt_timestamp".to_string(),
                AdditionalContextEntry {
                    value: prompt_submitted_at.to_string(),
                    kind: AdditionalContextKind::Application,
                },
            )])
        );
    }
}
