//! Request ownership and handoff decisions, without capture, model or window I/O.
//!
//! The coordinator serializes decisions and their display commits with its
//! coordination gate. This state is locked only for the decision itself; the
//! latest display snapshot remains owned by LatestCaptureStore.
use crate::{
    capture_session::{CaptureRequest, CaptureSession},
    foreground_context::ForegroundContext,
    model_config::ModelConfig,
    models::{CapturePayload, CapturePhase, CapturedSelection},
    translation::{ContentType, TranslationRequest},
};

pub enum TriggerAction {
    Capture(CaptureRequest),
    Probe(CaptureRequest),
}

pub enum SelectionAction {
    Stale,
    Reuse,
    Start(CaptureRequest),
}

pub enum RetryAction {
    Ignore,
    Capture(CaptureRequest),
    Translate(CaptureRequest, TranslationRequest),
}

#[derive(Default)]
pub struct TranslationTask {
    sessions: CaptureSession,
    probes: CaptureSession,
    active: Option<ActiveTranslation>,
    retry_source: Option<(u64, TranslationRequest)>,
}

struct ActiveTranslation {
    request: CaptureRequest,
    source: TranslationRequest,
    config: ModelConfig,
    context: ForegroundContext,
}

impl ActiveTranslation {
    fn matches(&self, selection: &CapturedSelection, config: &ModelConfig) -> bool {
        let normalized = TranslationRequest::new(
            selection.text.clone(),
            config.mode,
            selection.foreground_context.application_name.clone(),
            ContentType::Unknown,
        )
        .with_academic_context(
            selection.context_before.clone(),
            selection.context_after.clone(),
        );
        self.context.hwnd == selection.foreground_context.hwnd
            && self.context.process_id == selection.foreground_context.process_id
            && self.config == *config
            && self.source.source_text == selection.text
            && self.source.context_before == normalized.context_before
            && self.source.context_after == normalized.context_after
    }
}

impl TranslationTask {
    pub fn is_current(&self, request: &CaptureRequest) -> bool {
        self.sessions.is_current(request)
    }

    fn begin(&mut self) -> CaptureRequest {
        self.probes.cancel_current();
        self.active = None;
        self.retry_source = None;
        self.sessions.begin()
    }

    pub fn cancel(&mut self) {
        self.sessions.cancel_current();
        self.probes.cancel_current();
        self.active = None;
        self.retry_source = None;
    }

    fn active_for(&self, latest: Option<&CapturePayload>) -> Option<&ActiveTranslation> {
        self.active.as_ref().filter(|active| {
            self.is_current(&active.request)
                && latest.is_some_and(|payload| payload.request_id == active.request.id)
        })
    }

    pub fn trigger(&mut self, latest: Option<&CapturePayload>) -> TriggerAction {
        if self.active_for(latest).is_some()
            && latest.is_some_and(|payload| payload.phase == CapturePhase::Translating)
        {
            TriggerAction::Probe(self.probes.begin())
        } else {
            TriggerAction::Capture(self.begin())
        }
    }

    pub fn selection(
        &mut self,
        selection: &CapturedSelection,
        config: &ModelConfig,
        latest: Option<&CapturePayload>,
        probe: Option<&CaptureRequest>,
    ) -> SelectionAction {
        if probe.is_some_and(|request| !self.probes.is_current(request)) {
            return SelectionAction::Stale;
        }
        let duplicate = self.active_for(latest).is_some_and(|active| {
            active.matches(selection, config)
                && latest.is_some_and(|payload| {
                    payload.phase == CapturePhase::Translating
                        || (probe.is_some() && payload.phase == CapturePhase::Translated)
                })
        });
        self.probes.cancel_current();
        if duplicate {
            SelectionAction::Reuse
        } else {
            SelectionAction::Start(self.begin())
        }
    }

    pub fn started(
        &mut self,
        request: &CaptureRequest,
        source: TranslationRequest,
        config: ModelConfig,
        context: ForegroundContext,
    ) {
        if self.is_current(request) {
            self.active = Some(ActiveTranslation {
                request: request.clone(),
                source,
                config,
                context,
            });
        }
    }

    /// Validate again at the commit boundary. A late failure must not replace
    /// the retry source of the current request. Success keeps active identity
    /// so a probe begun during translation can still reuse the displayed result.
    pub fn complete(
        &mut self,
        request: &CaptureRequest,
        failed_source: Option<&TranslationRequest>,
    ) -> bool {
        if !self.is_current(request) {
            return false;
        }
        self.retry_source = failed_source.map(|source| (request.id, source.clone()));
        true
    }

    pub fn retry(
        &mut self,
        request_id: u64,
        latest: Option<&CapturePayload>,
    ) -> Result<RetryAction, &'static str> {
        let Some(payload) = latest.filter(|payload| payload.request_id == request_id) else {
            return Ok(RetryAction::Ignore);
        };
        match payload.phase {
            CapturePhase::CaptureFailed => Ok(RetryAction::Capture(self.begin())),
            CapturePhase::TranslationFailed => {
                if !self
                    .retry_source
                    .as_ref()
                    .is_some_and(|(id, _)| *id == request_id)
                {
                    return Err("原文已不可用，请重新选择文字");
                }
                let (_, source) = self.retry_source.take().unwrap();
                Ok(RetryAction::Translate(self.begin(), source))
            }
            _ => Ok(RetryAction::Ignore),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{latest_capture_store::LatestCaptureStore, translation::TranslationMode};

    // Drives the same decisions as the coordinator, with synthetic capture/model
    // completions. Counters stand in for model starts and loading/result delivery.
    #[derive(Default)]
    struct Harness {
        task: TranslationTask,
        display: LatestCaptureStore,
        starts: Vec<TranslationRequest>,
        loading: usize,
        commits: usize,
    }

    fn config() -> ModelConfig {
        ModelConfig {
            mode: TranslationMode::Academic,
            ..ModelConfig::default()
        }
    }

    fn selection(text: &str) -> CapturedSelection {
        CapturedSelection {
            text: text.into(),
            context_before: Some("Before".into()),
            context_after: Some("After".into()),
            foreground_context: ForegroundContext {
                hwnd: Default::default(),
                process_id: 42,
                application_name: "reader".into(),
            },
            source: "fake capture".into(),
            selection_rect: None,
            elapsed_ms: 0,
            clipboard_restored: None,
            warning_code: None,
        }
    }

    fn payload(id: u64, phase: CapturePhase) -> CapturePayload {
        CapturePayload {
            request_id: id,
            phase,
            success: phase == CapturePhase::Translated,
            text: String::new(),
            application_name: "reader".into(),
            process_id: 42,
            capture_method: "fake capture".into(),
            elapsed_ms: 0,
            selection_rect: None,
            error_code: None,
            error_message: None,
            focus_preserved: true,
            clipboard_restored: None,
            warning_code: None,
            language_profile: None,
            translation_mode: Some(TranslationMode::Academic),
            tone_note: None,
        }
    }

    impl Harness {
        fn trigger(&mut self) -> TriggerAction {
            let action = self.task.trigger(self.display.get().as_ref());
            if let TriggerAction::Capture(request) = &action {
                self.display.begin_request(request.id);
            }
            action
        }

        fn probe(&mut self) -> CaptureRequest {
            let TriggerAction::Probe(probe) = self.trigger() else {
                panic!("expected probe")
            };
            probe
        }

        fn start(
            &mut self,
            request: &CaptureRequest,
            selected: &CapturedSelection,
            config: &ModelConfig,
            source: TranslationRequest,
        ) {
            if !self.task.is_current(request) {
                return;
            }
            self.task.started(
                request,
                source.clone(),
                config.clone(),
                selected.foreground_context.clone(),
            );
            self.starts.push(source);
            self.loading += 1;
            assert!(
                self.display
                    .store(payload(request.id, CapturePhase::Translating))
            );
        }

        fn captured(
            &mut self,
            request: &CaptureRequest,
            selected: &CapturedSelection,
            config: &ModelConfig,
        ) {
            let source = TranslationRequest::new(
                selected.text.clone(),
                config.mode,
                "reader",
                ContentType::Unknown,
            )
            .with_academic_context(
                selected.context_before.clone(),
                selected.context_after.clone(),
            );
            self.start(request, selected, config, source);
        }

        fn first(&mut self, selected: &CapturedSelection, config: &ModelConfig) -> CaptureRequest {
            let TriggerAction::Capture(request) = self.trigger() else {
                panic!("first capture must not probe")
            };
            self.captured(&request, selected, config);
            request
        }

        fn selected(
            &mut self,
            selected: &CapturedSelection,
            config: &ModelConfig,
            probe: Option<&CaptureRequest>,
        ) -> SelectionAction {
            let action = self
                .task
                .selection(selected, config, self.display.get().as_ref(), probe);
            if let SelectionAction::Start(request) = &action {
                self.display.begin_request(request.id);
                self.captured(request, selected, config);
            }
            action
        }

        fn probe_result(
            &mut self,
            probe: &CaptureRequest,
            result: Result<CapturedSelection, ()>,
            config: &ModelConfig,
        ) {
            if let Ok(selected) = result {
                self.selected(&selected, config, Some(probe));
            }
        }

        fn complete(
            &mut self,
            request: &CaptureRequest,
            source: &TranslationRequest,
            result: Result<&str, ()>,
        ) {
            if !self
                .task
                .complete(request, result.as_ref().err().map(|_| source))
            {
                return;
            }
            let mut next = payload(
                request.id,
                if result.is_ok() {
                    CapturePhase::Translated
                } else {
                    CapturePhase::TranslationFailed
                },
            );
            next.text = result.unwrap_or_default().into();
            assert!(self.display.store(next));
            self.commits += 1;
        }

        fn retry(&mut self, id: u64) -> Result<Option<CaptureRequest>, &'static str> {
            match self.task.retry(id, self.display.get().as_ref())? {
                RetryAction::Ignore => Ok(None),
                RetryAction::Capture(request) => {
                    self.display.begin_request(request.id);
                    Ok(Some(request))
                }
                RetryAction::Translate(request, source) => {
                    self.display.begin_request(request.id);
                    self.start(
                        &request,
                        &selection("later selected text"),
                        &config(),
                        source,
                    );
                    Ok(Some(request))
                }
            }
        }
    }

    #[test]
    fn repeated_trigger_and_icon_reuse_without_model_start_or_loading_redraw() {
        let mut h = Harness::default();
        let selected = selection("original");
        let current = h.first(&selected, &config());
        let probe = h.probe();
        let mut equivalent = selected.clone();
        equivalent.context_before = Some("  Before\n".into());
        assert!(matches!(
            h.selected(&equivalent, &config(), Some(&probe)),
            SelectionAction::Reuse
        ));
        assert!(matches!(
            h.selected(&selected, &config(), None),
            SelectionAction::Reuse
        ));
        assert_eq!((h.starts.len(), h.loading), (1, 1));
        assert!(h.task.is_current(&current));

        let mut h = Harness::default();
        let mut empty = selected.clone();
        empty.context_before = None;
        h.first(&empty, &config());
        empty.context_before = Some(" \r\n ".into());
        assert!(matches!(
            h.selected(&empty, &config(), None),
            SelectionAction::Reuse
        ));
        let daily = ModelConfig::default();
        let mut h = Harness::default();
        h.first(&selected, &daily);
        empty.context_after = Some("irrelevant in daily mode".into());
        assert!(matches!(
            h.selected(&empty, &daily, None),
            SelectionAction::Reuse
        ));
        assert_eq!(h.starts.len(), 1);
    }

    #[test]
    fn changed_source_context_window_process_or_configuration_starts_new_task() {
        for change in 0..10 {
            let mut h = Harness::default();
            let mut selected = selection("original");
            let mut cfg = config();
            let old = h.first(&selected, &cfg);
            let probe = h.probe();
            match change {
                0 => selected.text = "new".into(),
                1 => selected.context_before = Some("other".into()),
                2 => selected.context_after = Some("other".into()),
                3 => selected.foreground_context.process_id += 1,
                4 => {
                    selected.foreground_context.hwnd =
                        windows::Win32::Foundation::HWND(1usize as *mut _)
                }
                5 => cfg.mode = TranslationMode::Conversational,
                6 => cfg.backend = crate::model_config::ModelBackend::Api,
                7 => cfg.local.model = "other".into(),
                8 => cfg.reasoning_enabled = !cfg.reasoning_enabled,
                _ => cfg.api.base_url = "https://example.invalid/v1".into(),
            }
            assert!(
                matches!(
                    h.selected(&selected, &cfg, Some(&probe)),
                    SelectionAction::Start(_)
                ),
                "change {change}"
            );
            assert_eq!((h.starts.len(), h.loading), (2, 2));
            assert!(!h.task.is_current(&old));
            assert!(old.cancellation.is_cancelled());
        }
    }

    #[test]
    fn completion_during_probe_keeps_result_but_later_active_trigger_translates_again() {
        let mut h = Harness::default();
        let selected = selection("original");
        let request = h.first(&selected, &config());
        let probe = h.probe();
        let source = h.starts[0].clone();
        h.complete(&request, &source, Ok("result"));
        h.probe_result(&probe, Ok(selected.clone()), &config());
        assert_eq!((h.starts.len(), h.loading, h.commits), (1, 1, 1));
        assert_eq!(h.display.get().unwrap().text, "result");
        let TriggerAction::Capture(next) = h.trigger() else {
            panic!("completed requests must not deduplicate")
        };
        h.captured(&next, &selected, &config());
        assert_eq!(h.starts.len(), 2);

        let mut h = Harness::default();
        let request = h.first(&selected, &config());
        let source = h.starts[0].clone();
        h.complete(&request, &source, Ok("result"));
        assert!(matches!(
            h.selected(&selected, &config(), None),
            SelectionAction::Start(_)
        ));
    }

    #[test]
    fn newer_probe_and_new_selection_reject_late_checks_and_model_results() {
        let mut h = Harness::default();
        let selected = selection("original");
        let old = h.first(&selected, &config());
        let old_source = h.starts[0].clone();
        let old_probe = h.probe();
        let latest_probe = h.probe();
        assert!(matches!(
            h.selected(&selection("stale check"), &config(), Some(&old_probe)),
            SelectionAction::Stale
        ));
        assert_eq!(h.starts.len(), 1);
        let SelectionAction::Start(new) =
            h.selected(&selection("new"), &config(), Some(&latest_probe))
        else {
            panic!("new selection")
        };
        let new_source = h.starts[1].clone();
        h.complete(&new, &new_source, Ok("new result"));
        h.complete(&old, &old_source, Ok("late result"));
        h.complete(&old, &old_source, Err(()));
        assert!(matches!(
            h.selected(&selected, &config(), Some(&latest_probe)),
            SelectionAction::Stale
        ));
        assert_eq!((h.starts.len(), h.commits), (2, 1));
        assert_eq!(h.display.get().unwrap().text, "new result");
        assert!(h.task.retry_source.is_none());
    }

    #[test]
    fn late_initial_capture_cannot_start_translation_after_new_trigger_or_cancel() {
        let mut h = Harness::default();
        let TriggerAction::Capture(old) = h.trigger() else {
            panic!()
        };
        let TriggerAction::Capture(new) = h.trigger() else {
            panic!()
        };
        h.captured(&old, &selection("old"), &config());
        h.captured(&new, &selection("new"), &config());
        assert_eq!(h.starts.len(), 1);
        assert_eq!(h.starts[0].source_text, "new");
        h.task.cancel();
        h.captured(&new, &selection("late"), &config());
        assert_eq!(h.starts.len(), 1);
    }

    #[test]
    fn failed_probe_preserves_running_translation_and_successful_result() {
        let mut h = Harness::default();
        let current = h.first(&selection("original"), &config());
        let probe = h.probe();
        h.probe_result(&probe, Err(()), &config());
        assert!(h.task.is_current(&current));
        assert_eq!(h.display.get().unwrap().phase, CapturePhase::Translating);
        let source = h.starts[0].clone();
        h.complete(&current, &source, Ok("result"));
        assert_eq!((h.starts.len(), h.loading, h.commits), (1, 1, 1));
    }

    #[test]
    fn late_completion_cannot_clear_or_replace_new_failure_retry_source() {
        let mut h = Harness::default();
        let old = h.first(&selection("old"), &config());
        let old_source = h.starts[0].clone();
        let probe = h.probe();
        // Failure during a probe must not be treated as a successfully completed duplicate.
        h.complete(&old, &old_source, Err(()));
        let SelectionAction::Start(new) = h.selected(&selection("new"), &config(), Some(&probe))
        else {
            panic!()
        };
        let new_source = h.starts[1].clone();
        h.complete(&new, &new_source, Err(()));
        h.complete(&old, &old_source, Ok("late success"));
        h.complete(&old, &old_source, Err(()));
        h.retry(new.id).unwrap().unwrap();
        assert_eq!(h.starts[2], new_source);
        assert_eq!(h.commits, 2);
    }

    #[test]
    fn retries_consume_retained_source_once_and_invalidate_old_probes() {
        let mut h = Harness::default();
        let current = h.first(&selection("original"), &config());
        let probe = h.probe();
        let source = h.starts[0].clone();
        h.complete(&current, &source, Err(()));
        assert!(h.retry(current.id + 99).unwrap().is_none());
        let retry = h.retry(current.id).unwrap().unwrap();
        assert_eq!(h.starts[1], source);
        assert_eq!(h.starts[1].context_before.as_deref(), Some("Before"));
        assert_eq!(h.starts[1].context_after.as_deref(), Some("After"));
        assert!(h.retry(current.id).unwrap().is_none());
        assert!(h.retry(retry.id).unwrap().is_none());
        h.probe_result(&probe, Ok(selection("later selected text")), &config());
        h.complete(&current, &source, Err(()));
        assert_eq!(h.starts.len(), 2);
        h.complete(&retry, &source, Err(()));
        let again = h.retry(retry.id).unwrap().unwrap();
        assert_eq!(h.starts[2], source);
        h.complete(&again, &source, Ok("recovered"));
        assert_eq!(h.display.get().unwrap().text, "recovered");
    }

    #[test]
    fn capture_retry_reserves_generation_before_another_retry_or_late_probe() {
        let mut h = Harness::default();
        let TriggerAction::Capture(request) = h.trigger() else {
            panic!()
        };
        h.display
            .store(payload(request.id, CapturePhase::CaptureFailed));
        let retry = h.retry(request.id).unwrap().unwrap();
        assert!(h.retry(request.id).unwrap().is_none());
        h.captured(&request, &selection("stale"), &config());
        h.captured(&retry, &selection("reread"), &config());
        assert_eq!(h.starts.len(), 1);
        assert_eq!(h.starts[0].source_text, "reread");
    }

    #[test]
    fn cancellation_rejects_probe_and_completion_and_clears_retry_source() {
        let mut h = Harness::default();
        let current = h.first(&selection("original"), &config());
        let probe = h.probe();
        let source = h.starts[0].clone();
        h.complete(&current, &source, Err(()));
        h.task.cancel();
        assert!(h.task.retry_source.is_none());
        assert!(h.retry(current.id).is_err());
        h.probe_result(&probe, Ok(selection("late")), &config());
        h.complete(&current, &source, Ok("late"));
        h.complete(&current, &source, Err(()));
        assert_eq!((h.starts.len(), h.loading, h.commits), (1, 1, 1));
        assert!(h.task.retry_source.is_none());
    }
}
