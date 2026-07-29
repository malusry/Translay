use serde::{Deserialize, Serialize};

pub const AUTO_SOURCE_LANGUAGE: &str = "auto";
pub const DEFAULT_TARGET_LANGUAGE: &str = "zh-CN";

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TranslationMode {
    Conversational,
    Academic,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ContentType {
    Unknown,
    Conversation,
    Academic,
    Technical,
    Menu,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlossaryEntry {
    pub source: String,
    pub target: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageProfile {
    /// A cheap local hint, not a definitive language detection result.
    ///
    /// `ja`, `ko` and `zh` are script-backed language hints. Values prefixed
    /// with `und-` identify only a Unicode script and must be resolved by the
    /// translation provider.
    pub language_hint: Option<String>,
    pub dominant_script: Option<String>,
    pub mixed_scripts: bool,
    pub requires_provider_detection: bool,
}

impl LanguageProfile {
    pub fn analyze(text: &str) -> Self {
        let counts = ScriptCounts::from_text(text);
        let (language_hint, dominant_script) = counts.dominant_hint();
        Self {
            requires_provider_detection: language_hint
                .as_deref()
                .is_none_or(|hint| hint.starts_with("und-")),
            mixed_scripts: counts.effective_group_count() > 1,
            language_hint,
            dominant_script,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationRequest {
    pub source_text: String,
    pub source_language: String,
    pub detected_language: Option<String>,
    pub language_profile: LanguageProfile,
    pub target_language: String,
    pub mode: TranslationMode,
    pub application: String,
    pub content_type: ContentType,
    pub domain: Option<String>,
    pub glossary: Vec<GlossaryEntry>,
    pub preserve_format: bool,
}

impl TranslationRequest {
    pub fn new(
        source_text: impl Into<String>,
        mode: TranslationMode,
        application: impl Into<String>,
        content_type: ContentType,
    ) -> Self {
        let source_text = source_text.into();
        Self {
            language_profile: LanguageProfile::analyze(&source_text),
            source_text,
            source_language: AUTO_SOURCE_LANGUAGE.to_owned(),
            detected_language: None,
            target_language: DEFAULT_TARGET_LANGUAGE.to_owned(),
            mode,
            application: application.into(),
            content_type,
            domain: None,
            glossary: Vec::new(),
            preserve_format: true,
        }
    }
}

#[derive(Default)]
struct ScriptCounts {
    han: usize,
    hiragana: usize,
    katakana: usize,
    hangul: usize,
    latin: usize,
    cyrillic: usize,
    arabic: usize,
    hebrew: usize,
}

impl ScriptCounts {
    fn from_text(text: &str) -> Self {
        let mut counts = Self::default();
        for character in text.chars() {
            let code = character as u32;
            if is_han(code) {
                counts.han += 1;
            } else if in_ranges(code, &[(0x3040, 0x309F)]) {
                counts.hiragana += 1;
            } else if in_ranges(
                code,
                &[(0x30A0, 0x30FF), (0x31F0, 0x31FF), (0xFF66, 0xFF9D)],
            ) {
                counts.katakana += 1;
            } else if in_ranges(
                code,
                &[
                    (0x1100, 0x11FF),
                    (0x3130, 0x318F),
                    (0xA960, 0xA97F),
                    (0xAC00, 0xD7AF),
                    (0xD7B0, 0xD7FF),
                ],
            ) {
                counts.hangul += 1;
            } else if character.is_ascii_alphabetic()
                || in_ranges(
                    code,
                    &[(0x00C0, 0x024F), (0x1E00, 0x1EFF), (0xAB30, 0xAB6F)],
                )
            {
                counts.latin += 1;
            } else if in_ranges(
                code,
                &[(0x0400, 0x052F), (0x2DE0, 0x2DFF), (0xA640, 0xA69F)],
            ) {
                counts.cyrillic += 1;
            } else if in_ranges(
                code,
                &[
                    (0x0600, 0x06FF),
                    (0x0750, 0x077F),
                    (0x08A0, 0x08FF),
                    (0xFB50, 0xFDFF),
                    (0xFE70, 0xFEFF),
                ],
            ) {
                counts.arabic += 1;
            } else if in_ranges(code, &[(0x0590, 0x05FF), (0xFB1D, 0xFB4F)]) {
                counts.hebrew += 1;
            }
        }
        counts
    }

    fn dominant_hint(&self) -> (Option<String>, Option<String>) {
        let kana = self.hiragana + self.katakana;
        if kana > 0 {
            return (Some("ja".to_owned()), Some("japanese".to_owned()));
        }
        if self.hangul > 0 {
            return (Some("ko".to_owned()), Some("hangul".to_owned()));
        }

        let candidates = [
            (self.han, "zh", "han"),
            (self.latin, "und-Latn", "latin"),
            (self.cyrillic, "und-Cyrl", "cyrillic"),
            (self.arabic, "und-Arab", "arabic"),
            (self.hebrew, "und-Hebr", "hebrew"),
        ];
        candidates
            .into_iter()
            .filter(|(count, _, _)| *count > 0)
            .max_by_key(|(count, _, _)| *count)
            .map_or((None, None), |(_, hint, script)| {
                (Some(hint.to_owned()), Some(script.to_owned()))
            })
    }

    fn effective_group_count(&self) -> usize {
        let kana = self.hiragana + self.katakana;
        let east_asian = if kana > 0 {
            1 + usize::from(self.hangul > 0)
        } else {
            usize::from(self.hangul > 0 || self.han > 0)
        };
        east_asian
            + usize::from(self.latin > 0)
            + usize::from(self.cyrillic > 0)
            + usize::from(self.arabic > 0)
            + usize::from(self.hebrew > 0)
    }
}

fn is_han(code: u32) -> bool {
    in_ranges(
        code,
        &[
            (0x3400, 0x4DBF),
            (0x4E00, 0x9FFF),
            (0xF900, 0xFAFF),
            (0x20000, 0x2FA1F),
        ],
    )
}

fn in_ranges(code: u32, ranges: &[(u32, u32)]) -> bool {
    ranges
        .iter()
        .any(|(start, end)| (*start..=*end).contains(&code))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifies_japanese_without_treating_kanji_and_kana_as_mixed() {
        let profile = LanguageProfile::analyze("人工知能について学びます。");
        assert_eq!(profile.language_hint.as_deref(), Some("ja"));
        assert_eq!(profile.dominant_script.as_deref(), Some("japanese"));
        assert!(!profile.mixed_scripts);
        assert!(!profile.requires_provider_detection);
    }

    #[test]
    fn identifies_korean() {
        let profile = LanguageProfile::analyze("인공지능에 대해 공부합니다.");
        assert_eq!(profile.language_hint.as_deref(), Some("ko"));
        assert!(!profile.mixed_scripts);
    }

    #[test]
    fn identifies_chinese_as_a_hint_without_skipping_translation() {
        let profile = LanguageProfile::analyze("这是一个中文句子。");
        assert_eq!(profile.language_hint.as_deref(), Some("zh"));
        assert!(!profile.requires_provider_detection);
    }

    #[test]
    fn script_only_hints_require_provider_detection() {
        for (text, expected) in [
            ("A multilingual façade.", "und-Latn"),
            ("Это русский текст.", "und-Cyrl"),
            ("هذا نص عربي.", "und-Arab"),
            ("זה טקסט בעברית.", "und-Hebr"),
        ] {
            let profile = LanguageProfile::analyze(text);
            assert_eq!(profile.language_hint.as_deref(), Some(expected));
            assert!(profile.requires_provider_detection);
        }
    }

    #[test]
    fn marks_japanese_and_latin_text_as_mixed() {
        let profile = LanguageProfile::analyze("この API returns JSON.");
        assert_eq!(profile.language_hint.as_deref(), Some("ja"));
        assert!(profile.mixed_scripts);
    }

    #[test]
    fn marks_japanese_and_korean_as_mixed_languages() {
        let profile = LanguageProfile::analyze("日本語と한국어");
        assert!(profile.mixed_scripts);
    }

    #[test]
    fn translation_request_is_language_agnostic_and_preserves_unicode() {
        let source = "한국어와 English가 섞인 문장입니다.";
        let request = TranslationRequest::new(
            source,
            TranslationMode::Academic,
            "msedge",
            ContentType::Academic,
        );
        assert_eq!(request.source_text, source);
        assert_eq!(request.source_language, AUTO_SOURCE_LANGUAGE);
        assert_eq!(request.target_language, DEFAULT_TARGET_LANGUAGE);
        assert_eq!(request.mode, TranslationMode::Academic);
        assert!(request.preserve_format);
        assert!(request.language_profile.mixed_scripts);

        let serialized = serde_json::to_string(&request).unwrap();
        assert!(serialized.contains(source));
        assert!(serialized.contains("\"sourceLanguage\":\"auto\""));
        assert!(serialized.contains("\"targetLanguage\":\"zh-CN\""));
    }
}
