use std::sync::{Arc, Mutex};

use crate::models::CapturePayload;

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

    pub fn clear_if_request(&self, request_id: u64) -> bool {
        let mut state = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        if state
            .payload
            .as_ref()
            .is_some_and(|payload| payload.request_id == request_id)
        {
            state.payload = None;
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
}
