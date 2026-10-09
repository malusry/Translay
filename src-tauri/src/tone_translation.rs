/// A translation and its optional, deliberately secondary conversational note.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranslationOutput {
    pub text: String,
    pub tone_note: Option<String>,
}

impl From<String> for TranslationOutput {
    fn from(text: String) -> Self {
        Self {
            text,
            tone_note: None,
        }
    }
}

pub fn parse_conversational_output(output: &str) -> Result<TranslationOutput, String> {
    let output = output.trim();
    let json = output
        .strip_prefix("```json")
        .or_else(|| output.strip_prefix("```"))
        .and_then(|s| s.trim().strip_suffix("```"))
        .map(str::trim)
        .unwrap_or(output);
    match serde_json::from_str::<serde_json::Value>(json) {
        Ok(value) => {
            let text = value
                .get("translation")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| "模型未返回有效译文，请重试".to_owned())?;
            // Invalid optional notes never discard an otherwise valid translation.
            let tone_note = value
                .get("toneNote")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| {
                    !s.is_empty()
                        && s.chars().count() <= 48
                        && !s.contains(['\n', '\r'])
                        && *s != text
                })
                .map(str::to_owned);
            Ok(TranslationOutput {
                text: text.to_owned(),
                tone_note,
            })
        }
        Err(_) if json.starts_with('{') || json.starts_with('[') => {
            // Never expose a broken response envelope as the visible translation.
            Err("模型返回的译文格式不完整，请重试".to_owned())
        }
        Err(_) if output.is_empty() => Err("模型返回了空译文".to_owned()),
        // Older/local providers may still obey the former plain-text contract.
        Err(_) => Ok(output.to_owned().into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_translation_separate_and_preserves_paragraphs() {
        let result = parse_conversational_output(
            r#"{"translation":"有道理。\n下一段。","toneNote":"口语中表示认可对方的判断。"}"#,
        )
        .unwrap();
        assert_eq!(result.text, "有道理。\n下一段。");
        assert_eq!(
            result.tone_note.as_deref(),
            Some("口语中表示认可对方的判断。")
        );
    }

    #[test]
    fn optional_note_failure_does_not_lose_translation() {
        for note in [
            serde_json::Value::Null,
            serde_json::json!(42),
            serde_json::json!(""),
            serde_json::json!("译文"),
            serde_json::json!("一\n二"),
            serde_json::json!("字".repeat(49)),
        ] {
            let input = serde_json::json!({"translation":"译文", "toneNote":note});
            let result = parse_conversational_output(&input.to_string()).unwrap();
            assert_eq!(result.text, "译文");
            assert_eq!(result.tone_note, None);
        }
        assert_eq!(
            parse_conversational_output(r#"{"translation":"译文"}"#)
                .unwrap()
                .tone_note,
            None
        );
    }

    #[test]
    fn accepts_fenced_json_and_legacy_plain_text() {
        assert_eq!(
            parse_conversational_output(
                "```json\n{\"translation\":\"有道理。\",\"toneNote\":null}\n```"
            )
            .unwrap()
            .text,
            "有道理。"
        );
        assert_eq!(
            parse_conversational_output("1. [名词] 路径\n2. [动词] 追踪")
                .unwrap()
                .tone_note,
            None
        );
    }

    #[test]
    fn rejects_broken_envelopes_and_missing_translation() {
        for output in [
            "",
            "{\"translation\":",
            "{}",
            "[]",
            "{\"translation\":42}",
            "{\"translation\":\" \"}",
        ] {
            assert!(parse_conversational_output(output).is_err(), "{output}");
        }
    }
}
