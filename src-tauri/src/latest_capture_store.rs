use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

use crate::{
    explanation::{ExplanationContent, ExplanationRequest},
    models::{CapturePayload, CapturePhase},
    translation::TranslationMode,
};

#[derive(Clone, Default)]
pub struct LatestCaptureStore {
    inner: Arc<Mutex<LatestCaptureState>>,
}

#[derive(Default)]
struct LatestCaptureState {
    latest_request_id: u64,
    shown_request_id: Option<u64>,
    acknowledged_request_id: Option<u64>,
    payload: Option<CapturePayload>,
    explanation_request: Option<ExplanationRequest>,
    explanation: Option<ExplanationContent>,
    explanation_attempt_id: u64,
    explanation_cancel: Option<oneshot::Sender<()>>,
}

impl LatestCaptureState {
    fn cancel_explanation(&mut self) {
        if let Some(cancel) = self.explanation_cancel.take() {
            let _ = cancel.send(());
        }
    }

    fn reset_explanation(&mut self) {
        self.cancel_explanation();
        self.explanation_attempt_id = 0;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AckStatus {
    Accepted,
    AlreadyAcknowledged,
    Rejected,
}

impl LatestCaptureStore {
    /// Marks a new generation before capture begins. Dropping the previous
    /// payload also shortens the lifetime of source text in Rust memory.
    pub fn begin_request(&self, request_id: u64) {
        let mut state = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        if request_id >= state.latest_request_id {
            state.latest_request_id = request_id;
            state.shown_request_id = None;
            state.acknowledged_request_id = None;
            state.payload = None;
            state.reset_explanation();
            state.explanation_request = None;
            state.explanation = None;
        }
    }

    /// Stages a payload only when it belongs to the newest known generation.
    pub fn store(&self, payload: CapturePayload) -> bool {
        let mut state = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        if payload.request_id < state.latest_request_id {
            return false;
        }
        if state
            .payload
            .as_ref()
            .is_some_and(|current| payload.request_id < current.request_id)
        {
            return false;
        }
        if payload.request_id > state.latest_request_id {
            state.reset_explanation();
            state.explanation_request = None;
            state.explanation = None;
        }
        state.latest_request_id = payload.request_id;
        state.acknowledged_request_id = None;
        state.payload = Some(payload);
        true
    }

    pub fn mark_shown(&self, request_id: u64) -> bool {
        let mut state = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        let is_current = state.latest_request_id == request_id
            && state
                .payload
                .as_ref()
                .is_some_and(|payload| payload.request_id == request_id);
        if !is_current {
            return false;
        }
        state.shown_request_id = Some(request_id);
        state.acknowledged_request_id = None;
        true
    }

    pub fn acknowledge(&self, request_id: u64) -> AckStatus {
        let mut state = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        let is_current_shown = state.latest_request_id == request_id
            && state.shown_request_id == Some(request_id)
            && state
                .payload
                .as_ref()
                .is_some_and(|payload| payload.request_id == request_id);
        if !is_current_shown {
            return AckStatus::Rejected;
        }
        if state.acknowledged_request_id == Some(request_id) {
            return AckStatus::AlreadyAcknowledged;
        }
        state.acknowledged_request_id = Some(request_id);
        AckStatus::Accepted
    }

    #[cfg(test)]
    pub fn is_acknowledged(&self, request_id: u64) -> bool {
        let state = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        state.latest_request_id == request_id
            && state.shown_request_id == Some(request_id)
            && state.acknowledged_request_id == Some(request_id)
    }

    pub fn is_current_unacknowledged(&self, request_id: u64) -> bool {
        let state = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        state.latest_request_id == request_id
            && state.shown_request_id == Some(request_id)
            && state
                .payload
                .as_ref()
                .is_some_and(|payload| payload.request_id == request_id)
            && state.acknowledged_request_id != Some(request_id)
    }

    /// The command recovery path only returns a payload that still matches the
    /// latest request generation.
    pub fn get(&self) -> Option<CapturePayload> {
        let state = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        state
            .payload
            .as_ref()
            .filter(|payload| payload.request_id == state.latest_request_id)
            .cloned()
    }

    pub fn store_explanation_request(&self, request: ExplanationRequest) -> bool {
        let mut state = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        let eligible = state.latest_request_id == request.request_id
            && state.payload.as_ref().is_some_and(|payload| {
                payload.request_id == request.request_id
                    && payload.phase == CapturePhase::Translated
                    && payload.success
                    && payload.translation_mode == Some(TranslationMode::Academic)
            });
        if !eligible {
            return false;
        }
        state.explanation_request = Some(request);
        state.explanation = None;
        true
    }

    pub fn begin_explanation(
        &self,
        request_id: u64,
        attempt_id: u64,
    ) -> Option<(ExplanationRequest, oneshot::Receiver<()>)> {
        let mut state = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        if state.latest_request_id != request_id || attempt_id <= state.explanation_attempt_id {
            return None;
        }
        let request = state.explanation_request.as_ref()?.clone();
        state.cancel_explanation();
        let (cancel, cancelled) = oneshot::channel();
        state.explanation_attempt_id = attempt_id;
        state.explanation_cancel = Some(cancel);
        Some((request, cancelled))
    }

    pub fn cancel_explanation(&self, request_id: u64, attempt_id: u64) -> bool {
        let mut state = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        if state.latest_request_id != request_id || attempt_id < state.explanation_attempt_id {
            return false;
        }
        // Remember cancellation even if it arrives before the start command.
        state.explanation_attempt_id = attempt_id;
        state.cancel_explanation();
        true
    }

    pub fn finish_explanation(
        &self,
        request_id: u64,
        attempt_id: u64,
        content: Option<ExplanationContent>,
    ) -> bool {
        let mut state = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        if state.latest_request_id != request_id
            || state.explanation_attempt_id != attempt_id
            || state.explanation_cancel.is_none()
        {
            return false;
        }
        state.explanation_cancel = None;
        if let Some(content) = content {
            state.explanation = Some(content);
        }
        true
    }

    pub fn cached_explanation(&self, request_id: u64) -> Option<ExplanationContent> {
        let state = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        (state.latest_request_id == request_id)
            .then(|| state.explanation.as_ref())
            .flatten()
            .cloned()
    }

    pub fn clear_if_request(&self, request_id: u64) -> bool {
        let mut state = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        if state
            .payload
            .as_ref()
            .is_some_and(|payload| payload.request_id == request_id)
        {
            state.payload = None;
            state.reset_explanation();
            state.explanation_request = None;
            state.explanation = None;
            if state.shown_request_id == Some(request_id) {
                state.shown_request_id = None;
            }
            if state.acknowledged_request_id == Some(request_id) {
                state.acknowledged_request_id = None;
            }
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CapturePhase, ScreenRect};
    use crate::translation::{ContentType, TranslationRequest};

    fn payload(request_id: u64, text: &str) -> CapturePayload {
        CapturePayload {
            request_id,
            phase: CapturePhase::Translated,
            success: true,
            text: text.to_owned(),
            application_name: "test".to_owned(),
            process_id: 1,
            capture_method: "UIA:TextPattern".to_owned(),
            elapsed_ms: 10,
            selection_rect: Some(ScreenRect {
                left: 1,
                top: 2,
                right: 3,
                bottom: 4,
            }),
            error_code: None,
            error_message: None,
            focus_preserved: true,
            clipboard_restored: None,
            warning_code: None,
            language_profile: Some(crate::translation::LanguageProfile::analyze(text)),
            translation_mode: Some(crate::translation::TranslationMode::Conversational),
            tone_note: None,
        }
    }

    fn explanation_request(request_id: u64) -> ExplanationRequest {
        let request = TranslationRequest::new(
            "The result may depend on initialization.",
            TranslationMode::Academic,
            "reader",
            ContentType::Academic,
        );
        ExplanationRequest::from_translation(
            request_id,
            &request,
            "结果可能取决于初始化。".to_owned(),
        )
        .unwrap()
    }

    fn explanation() -> ExplanationContent {
        ExplanationContent {
            core_explanation: "这是一个带有限定强度的结论。".to_owned(),
            key_concepts: Vec::new(),
            caveat: String::new(),
        }
    }

    #[test]
    fn command_recovery_reads_event_that_happened_before_listener() {
        let store = LatestCaptureStore::default();
        store.begin_request(7);
        assert!(store.store(payload(7, "captured before mount")));

        let recovered = store.get().expect("latest capture should be recoverable");
        assert_eq!(recovered.request_id, 7);
        assert_eq!(recovered.text, "captured before mount");
    }

    #[test]
    fn newer_request_replaces_older_request() {
        let store = LatestCaptureStore::default();
        assert!(store.store(payload(1, "old")));
        assert!(store.store(payload(2, "new")));
        assert_eq!(store.get().unwrap().text, "new");
    }

    #[test]
    fn older_request_cannot_overwrite_newer_result() {
        let store = LatestCaptureStore::default();
        store.begin_request(9);
        assert!(store.store(payload(9, "new")));
        assert!(!store.store(payload(8, "late old")));
        assert_eq!(store.get().unwrap().text, "new");
    }

    #[test]
    fn hide_clears_only_the_matching_request() {
        let store = LatestCaptureStore::default();
        assert!(store.store(payload(12, "private text")));
        assert!(!store.clear_if_request(11));
        assert!(store.get().is_some());
        assert!(store.clear_if_request(12));
        assert!(store.get().is_none());
    }

    #[test]
    fn old_ack_cannot_affect_new_request() {
        let store = LatestCaptureStore::default();
        store.begin_request(30);
        assert!(store.store(payload(30, "new")));
        assert!(store.mark_shown(30));

        assert_eq!(store.acknowledge(29), AckStatus::Rejected);
        assert!(!store.is_acknowledged(30));
        assert_eq!(store.acknowledge(30), AckStatus::Accepted);
        assert!(store.is_acknowledged(30));
    }

    #[test]
    fn duplicate_ack_is_idempotent() {
        let store = LatestCaptureStore::default();
        store.begin_request(31);
        assert!(store.store(payload(31, "current")));
        assert!(store.mark_shown(31));

        assert_eq!(store.acknowledge(31), AckStatus::Accepted);
        assert_eq!(store.acknowledge(31), AckStatus::AlreadyAcknowledged);
    }

    #[test]
    fn unacknowledged_payload_survives_normal_reading_window() {
        let store = LatestCaptureStore::default();
        store.begin_request(32);
        assert!(store.store(payload(32, "not rendered yet")));
        assert!(store.mark_shown(32));

        assert!(store.is_current_unacknowledged(32));
        assert!(store.get().is_some());
    }

    #[test]
    fn stale_explanation_cannot_cross_into_a_new_translation() {
        let store = LatestCaptureStore::default();
        let mut academic_payload = payload(40, "译文");
        academic_payload.translation_mode = Some(TranslationMode::Academic);
        assert!(store.store(academic_payload));
        assert!(store.store_explanation_request(explanation_request(40)));

        store.begin_request(41);

        assert!(store.begin_explanation(40, 1).is_none());
        assert!(!store.finish_explanation(40, 1, Some(explanation())));
        assert!(store.cached_explanation(40).is_none());
    }

    #[test]
    fn current_explanation_is_cached_until_its_capture_is_cleared() {
        let store = LatestCaptureStore::default();
        let mut academic_payload = payload(42, "译文");
        academic_payload.translation_mode = Some(TranslationMode::Academic);
        assert!(store.store(academic_payload));
        assert!(store.store_explanation_request(explanation_request(42)));
        let _pending = store.begin_explanation(42, 1).unwrap();
        assert!(store.finish_explanation(42, 1, Some(explanation())));
        assert!(store.cached_explanation(42).is_some());

        assert!(store.clear_if_request(42));
        assert!(store.begin_explanation(42, 2).is_none());
        assert!(store.cached_explanation(42).is_none());
    }
    fn ready_for_explanation() -> LatestCaptureStore {
        let store = LatestCaptureStore::default();
        let mut translated = payload(50, "translation");
        translated.translation_mode = Some(TranslationMode::Academic);
        assert!(store.store(translated));
        assert!(store.store_explanation_request(explanation_request(50)));
        store
    }

    #[test]
    fn cancel_before_start_blocks_the_delayed_command_but_allows_reopen() {
        let store = ready_for_explanation();
        assert!(store.cancel_explanation(50, 100));
        assert!(store.begin_explanation(50, 100).is_none());
        assert!(store.begin_explanation(50, 101).is_some());
    }

    #[test]
    fn late_cancel_and_completion_cannot_affect_a_reopened_explanation() {
        let store = ready_for_explanation();
        let (_, mut old) = store.begin_explanation(50, 100).unwrap();
        assert!(store.cancel_explanation(50, 100));
        assert_eq!(old.try_recv(), Ok(()));
        let (_, mut current) = store.begin_explanation(50, 101).unwrap();
        assert!(!store.cancel_explanation(50, 100));
        assert!(!store.finish_explanation(50, 100, Some(explanation())));
        assert_eq!(current.try_recv(), Err(oneshot::error::TryRecvError::Empty));
        assert!(store.cached_explanation(50).is_none());
        assert!(store.finish_explanation(50, 101, Some(explanation())));
        assert!(store.cached_explanation(50).is_some());
    }

    #[test]
    fn new_capture_and_hide_cancel_pending_explanations() {
        let store = ready_for_explanation();
        let (_, mut pending) = store.begin_explanation(50, 100).unwrap();
        store.begin_request(51);
        assert_eq!(pending.try_recv(), Ok(()));
        assert!(!store.finish_explanation(50, 100, Some(explanation())));
        let store = ready_for_explanation();
        let (_, mut pending) = store.begin_explanation(50, 100).unwrap();
        assert!(store.clear_if_request(50));
        assert_eq!(pending.try_recv(), Ok(()));
        assert!(!store.finish_explanation(50, 100, Some(explanation())));
    }
}
