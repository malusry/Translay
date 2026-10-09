use serde::{Deserialize, Serialize};

use crate::translation::{TranslationMode, TranslationRequest};

/// Cancellation wins a simultaneous completion; returning drops the HTTP future.
pub async fn until_cancelled<T>(
    mut cancelled: tokio::sync::oneshot::Receiver<()>,
    operation: impl std::future::Future<Output = T>,
) -> Option<T> {
    use std::{future::Future, pin::Pin, task::Poll};
    let mut operation = std::pin::pin!(operation);
    std::future::poll_fn(|cx| {
        if Pin::new(&mut cancelled).poll(cx).is_ready() {
            return Poll::Ready(None);
        }
        operation.as_mut().poll(cx).map(Some)
    })
    .await
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExplanationRequest {
    pub request_id: u64,
    pub source_text: String,
    pub translation: String,
    pub context_before: Option<String>,
    pub context_after: Option<String>,
    pub language_hint: Option<String>,
}

impl ExplanationRequest {
    pub fn from_translation(
        request_id: u64,
        request: &TranslationRequest,
        translation: String,
    ) -> Option<Self> {
        (request.mode == TranslationMode::Academic).then(|| Self {
            request_id,
            source_text: request.source_text.clone(),
            translation,
            context_before: request.context_before.clone(),
            context_after: request.context_after.clone(),
            language_hint: request.language_profile.language_hint.clone(),
        })
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExplanationContent {
    pub core_explanation: String,
    #[serde(default)]
    pub key_concepts: Vec<ExplanationTerm>,
    #[serde(default)]
    pub caveat: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExplanationTerm {
    pub term: String,
    #[serde(default)]
    pub translation: String,
    pub explanation: String,
}

impl ExplanationContent {
    pub fn normalized(mut self) -> Result<Self, String> {
        self.core_explanation = self.core_explanation.trim().to_owned();
        if self.core_explanation.is_empty() {
            return Err("模型返回的解释缺少核心内容，请重试".to_owned());
        }
        self.caveat = self.caveat.trim().to_owned();
        if self.caveat == self.core_explanation {
            self.caveat.clear();
        }
        let mut seen = std::collections::HashSet::new();
        self.key_concepts = self
            .key_concepts
            .into_iter()
            .filter_map(|term| {
                let term_text = term.term.trim().to_owned();
                let translation = term.translation.trim().to_owned();
                let explanation = term.explanation.trim().to_owned();
                (!term_text.is_empty()
                    && !explanation.is_empty()
                    && seen.insert(term_text.to_lowercase()))
                .then_some(ExplanationTerm {
                    term: term_text,
                    translation,
                    explanation,
                })
            })
            .take(2)
            .collect();
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translation::ContentType;

    #[test]
    fn explanation_source_is_available_only_for_learning_mode() {
        let academic = TranslationRequest::new(
            "The evidence suggests an association.",
            TranslationMode::Academic,
            "reader",
            ContentType::Academic,
        )
        .with_academic_context(Some("Earlier claim.".to_owned()), None);
        let daily = TranslationRequest::new(
            "That tracks.",
            TranslationMode::Conversational,
            "browser",
            ContentType::Conversation,
        );

        let source = ExplanationRequest::from_translation(7, &academic, "译文".to_owned())
            .expect("academic translation should be explainable");
        assert_eq!(source.context_before.as_deref(), Some("Earlier claim."));
        assert!(ExplanationRequest::from_translation(8, &daily, "译文".to_owned()).is_none());
    }

    #[test]
    fn normalization_hides_empty_and_duplicate_concepts_and_caps_at_two() {
        let term = |name: &str, explanation: &str| ExplanationTerm {
            term: name.to_owned(),
            translation: " 译名 ".to_owned(),
            explanation: explanation.to_owned(),
        };
        let content = ExplanationContent {
            core_explanation: "  核心解释。  ".to_owned(),
            key_concepts: vec![
                term("", "invalid"),
                term(" evidence ", " 观察依据。 "),
                term("EVIDENCE", "重复"),
                term("empty", " "),
                term("variance", "离散程度。"),
                term("third", "不应保留。"),
            ],
            caveat: "  无法确定符号含义。  ".to_owned(),
        }
        .normalized()
        .unwrap();
        assert_eq!(content.core_explanation, "核心解释。");
        assert_eq!(content.key_concepts.len(), 2);
        assert_eq!(content.key_concepts[0].term, "evidence");
        assert_eq!(content.key_concepts[0].translation, "译名");
        assert_eq!(content.key_concepts[0].explanation, "观察依据。");
        assert_eq!(content.key_concepts[1].term, "variance");
        assert_eq!(content.caveat, "无法确定符号含义。");
    }

    #[test]
    fn optional_fields_can_be_omitted_and_duplicate_caveat_is_hidden() {
        let minimal: ExplanationContent =
            serde_json::from_str(r#"{"coreExplanation":"简单解释。"}"#).unwrap();
        let normalized = minimal.normalized().unwrap();
        assert!(normalized.key_concepts.is_empty());
        assert!(normalized.caveat.is_empty());
        let duplicate = ExplanationContent {
            caveat: " 简单解释。 ".to_owned(),
            ..normalized
        }
        .normalized()
        .unwrap();
        assert!(duplicate.caveat.is_empty());
    }
    #[test]
    fn cancelling_drops_a_pending_operation_and_wins_ready_races() {
        use std::{
            future::Future,
            sync::{
                Arc,
                atomic::{AtomicBool, Ordering},
            },
            task::{Context, Poll, Waker},
        };
        struct DropFlag(Arc<AtomicBool>);
        impl Drop for DropFlag {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let dropped = Arc::new(AtomicBool::new(false));
        let guard = DropFlag(dropped.clone());
        let operation = async move {
            let _guard = guard;
            std::future::pending::<()>().await;
        };
        let (cancel, cancelled) = tokio::sync::oneshot::channel();
        let mut wait = Box::pin(until_cancelled(cancelled, operation));
        let mut cx = Context::from_waker(Waker::noop());
        assert!(matches!(wait.as_mut().poll(&mut cx), Poll::Pending));
        cancel.send(()).unwrap();
        assert!(matches!(wait.as_mut().poll(&mut cx), Poll::Ready(None)));
        assert!(dropped.load(Ordering::SeqCst));

        let (cancel, cancelled) = tokio::sync::oneshot::channel();
        cancel.send(()).unwrap();
        assert_eq!(
            tauri::async_runtime::block_on(until_cancelled(cancelled, async { 7 })),
            None
        );
        let (_cancel, cancelled) = tokio::sync::oneshot::channel();
        assert_eq!(
            tauri::async_runtime::block_on(until_cancelled(cancelled, async { 7 })),
            Some(7)
        );
    }
}
