use std::{
    sync::{Arc, Mutex, MutexGuard},
    thread,
    time::Instant,
};

use tauri::{AppHandle, Manager};
use tracing::{error, info, warn};

use crate::{
    capture_session::CaptureRequest,
    clipboard_service::{CLIPBOARD_METHOD, ClipboardService},
    config::UIA_TIMEOUT,
    explanation::ExplanationRequest,
    foreground_context::ForegroundContext,
    model_config::{ModelBackend, ModelConfigStore},
    models::{CapturePayload, CapturePhase, CapturedSelection},
    overlay_manager::{OverlayManager, cursor_anchor},
    selection_service::{SelectionFailure, SelectionService},
    translation::{ContentType, LanguageProfile, TranslationMode, TranslationRequest},
    translation_service::TranslationService,
    translation_task::{RetryAction, SelectionAction, TranslationTask, TriggerAction},
};

#[derive(Clone)]
pub struct CaptureCoordinator {
    overlay: OverlayManager,
    // Serializes handoff and display commits. Never held over selection I/O or await.
    coordination: Arc<Mutex<()>>,
    task: Arc<Mutex<TranslationTask>>,
}

#[derive(Debug)]
struct PipelineFailure {
    code: String,
    message: String,
    source: String,
    clipboard_restored: Option<bool>,
    warning_code: Option<String>,
}

#[derive(Debug)]
struct PreparedTranslation {
    request: TranslationRequest,
    backend: ModelBackend,
}

impl CaptureCoordinator {
    pub fn new(overlay: OverlayManager) -> Self {
        Self {
            overlay,
            coordination: Arc::new(Mutex::new(())),
            task: Arc::new(Mutex::new(TranslationTask::default())),
        }
    }

    pub fn overlay(&self) -> &OverlayManager {
        &self.overlay
    }

    fn task(&self) -> MutexGuard<'_, TranslationTask> {
        self.task.lock().unwrap_or_else(|error| error.into_inner())
    }

    pub fn cancel_current(&self) {
        let _coordination = self
            .coordination
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        self.task().cancel();
    }

    pub fn trigger(&self, app: AppHandle) {
        // Read a repeated selection without cancelling a model request or changing
        // the visible loading state. A different selection enters the normal path.
        let action = {
            let _guard = self.coordination.lock().unwrap_or_else(|e| e.into_inner());
            let latest = app
                .state::<crate::latest_capture_store::LatestCaptureStore>()
                .get();
            let action = self.task().trigger(latest.as_ref());
            if let TriggerAction::Capture(request) = &action {
                self.overlay.begin_request(request.id);
            }
            action
        };
        if let TriggerAction::Probe(probe) = action {
            let Ok(context) = ForegroundContext::capture() else {
                return;
            };
            let academic = app.state::<ModelConfigStore>().get().mode == TranslationMode::Academic;
            let coordinator = self.clone();
            thread::spawn(move || {
                if let Ok(selection) = run_pipeline(
                    context,
                    probe.cancellation.clone(),
                    Instant::now(),
                    academic,
                ) {
                    coordinator.trigger_captured_checked(app, selection, Some(&probe));
                }
                // A failed reread must not erase an already running translation.
            });
            return;
        }
        if let TriggerAction::Capture(request) = action {
            self.trigger_capture(app, request);
        }
    }

    fn trigger_capture(&self, app: AppHandle, request: CaptureRequest) {
        // The generation is already reserved atomically with the trigger/retry
        // decision. Capture and window work do not hold the task state lock.
        let context = match ForegroundContext::capture() {
            Ok(context) => context,
            Err(code) => {
                warn!(
                    application = "unknown",
                    capture_method = "UI Automation",
                    text_length = 0,
                    elapsed_ms = 0,
                    error_code = code,
                    "selection capture failed"
                );
                return;
            }
        };
        let capture_academic_context =
            app.state::<ModelConfigStore>().get().mode == TranslationMode::Academic;
        let initial_payload = CapturePayload {
            request_id: request.id,
            phase: CapturePhase::Capturing,
            success: false,
            text: String::new(),
            application_name: context.application_name.clone(),
            process_id: context.process_id,
            capture_method: "UI Automation".to_owned(),
            elapsed_ms: 0,
            selection_rect: None,
            error_code: None,
            error_message: None,
            focus_preserved: true,
            clipboard_restored: None,
            warning_code: None,
            language_profile: None,
            translation_mode: None,
            tone_note: None,
        };
        {
            let _coordination = self
                .coordination
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let foreground_unchanged = context.is_valid_and_foreground();
            if !can_commit(self.task().is_current(&request), foreground_unchanged) {
                return;
            }
            match self
                .overlay
                .show(&app, &initial_payload, &context, cursor_anchor())
            {
                Ok(true) => {}
                Ok(false) => {
                    let _ = self.overlay.hide(&app);
                    return;
                }
                Err(_) => return,
            }
        }
        let coordinator = self.clone();
        let application_name = context.application_name.clone();
        let spawn_result = thread::Builder::new()
            .name(format!("translay-capture-{}", request.id))
            .spawn(move || {
                let started = Instant::now();
                let result = run_pipeline(
                    context.clone(),
                    request.cancellation.clone(),
                    started,
                    capture_academic_context,
                );
                if !coordinator.task().is_current(&request) {
                    return;
                }

                let (mut payload, anchor, prepared_translation) = match result {
                    Ok(selection) => {
                        let language_profile = LanguageProfile::analyze(&selection.text);
                        let model_config = app.state::<ModelConfigStore>().get();
                        let translation_request = TranslationRequest::new(
                            selection.text.clone(),
                            model_config.mode,
                            selection.foreground_context.application_name.clone(),
                            ContentType::Unknown,
                        )
                        .with_academic_context(
                            selection.context_before.clone(),
                            selection.context_after.clone(),
                        );
                        info!(
                            application = %selection.foreground_context.application_name,
                            capture_method = %selection.source,
                            text_length = selection.text.chars().count(),
                            elapsed_ms = selection.elapsed_ms,
                            error_code = "",
                            "selection capture completed"
                        );
                        let anchor = selection.selection_rect.unwrap_or_else(cursor_anchor);
                        (
                            CapturePayload {
                                request_id: request.id,
                                phase: CapturePhase::Translating,
                                success: false,
                                text: String::new(),
                                application_name: selection
                                    .foreground_context
                                    .application_name
                                    .clone(),
                                process_id: selection.foreground_context.process_id,
                                capture_method: selection.source,
                                elapsed_ms: selection.elapsed_ms,
                                selection_rect: selection.selection_rect,
                                error_code: None,
                                error_message: None,
                                focus_preserved: true,
                                clipboard_restored: selection.clipboard_restored,
                                warning_code: selection.warning_code,
                                language_profile: Some(language_profile),
                                translation_mode: Some(model_config.mode),
                                tone_note: None,
                            },
                            anchor,
                            Some(PreparedTranslation {
                                request: translation_request,
                                backend: model_config.backend,
                            }),
                        )
                    }
                    Err(failure) => {
                        // Foreground changes are cancellation boundaries, not
                        // user-facing capture failures.
                        if failure.code == "FOREGROUND_CHANGED" {
                            warn!(
                                application = %context.application_name,
                                capture_method = %failure.source,
                                text_length = 0,
                                elapsed_ms = started.elapsed().as_millis(),
                                error_code = %failure.code,
                                "capture result discarded after foreground change"
                            );
                            return;
                        }
                        let elapsed = started.elapsed().as_millis();
                        warn!(
                            application = %context.application_name,
                            capture_method = %failure.source,
                            text_length = 0,
                            elapsed_ms = elapsed,
                            error_code = %failure.code,
                            "selection capture failed"
                        );
                        (
                            CapturePayload {
                                request_id: request.id,
                                phase: CapturePhase::CaptureFailed,
                                success: false,
                                text: String::new(),
                                application_name: context.application_name.clone(),
                                process_id: context.process_id,
                                capture_method: failure.source,
                                elapsed_ms: elapsed,
                                selection_rect: None,
                                error_code: Some(failure.code.clone()),
                                error_message: Some(if failure.code == "NO_TEXT_SELECTED" { "请先选择文字".into() } else { failure.message }),
                                focus_preserved: true,
                                clipboard_restored: failure.clipboard_restored,
                                warning_code: failure.warning_code,
                                language_profile: None,
                                translation_mode: None,
                                tone_note: None,
                            },
                            cursor_anchor(),
                            None,
                        )
                    }
                };

                // Close the final check/show race against another shortcut press.
                let _coordination = coordinator
                    .coordination
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                let foreground_unchanged = context.is_valid_and_foreground();
                if !can_commit(coordinator.task().is_current(&request), foreground_unchanged) {
                    return;
                }
                let mut local_display_gate = None;
                if let Some(prepared) = prepared_translation
                    .as_ref()
                    .filter(|prepared| should_prestart_translation(prepared.backend))
                {
                    let (display_ready, wait_for_display) = tauri::async_runtime::channel(1);
                    coordinator.start_translation(
                        app.clone(),
                        request.clone(),
                        payload.clone(),
                        prepared.request.clone(),
                        started,
                        Some(wait_for_display),
                    );
                    local_display_gate = Some(display_ready);
                }

                match coordinator.overlay.show(&app, &payload, &context, anchor) {
                    Ok(focus_preserved) => {
                        payload.focus_preserved = focus_preserved;
                        if !focus_preserved {
                            let _ = coordinator.overlay.hide(&app);
                            warn!(
                                application = %context.application_name,
                                capture_method = %payload.capture_method,
                                text_length = if payload.success { payload.text.chars().count() } else { 0 },
                                elapsed_ms = payload.elapsed_ms,
                                error_code = "FOCUS_CHANGED_AFTER_OVERLAY",
                                "overlay was hidden because focus verification failed"
                            );
                            return;
                        }
                    }
                    Err(_) => {
                        error!(
                            application = %context.application_name,
                            capture_method = %payload.capture_method,
                            text_length = if payload.success { payload.text.chars().count() } else { 0 },
                            elapsed_ms = payload.elapsed_ms,
                            error_code = "OVERLAY_SHOW_FAILED",
                            "overlay show failed"
                        );
                        return;
                    }
                }

                if let Some(display_ready) = local_display_gate {
                    // A very fast local model may finish while the native loading
                    // surface is still being positioned. Releasing this gate only
                    // after show succeeds preserves the existing state order.
                    let _ = display_ready.try_send(());
                } else if let Some(prepared) = prepared_translation {
                    coordinator.start_translation(
                        app.clone(),
                        request.clone(),
                        payload,
                        prepared.request,
                        started,
                        None,
                    );
                }
            });
        if spawn_result.is_err() {
            error!(
                application = %application_name,
                capture_method = "UI Automation",
                text_length = 0,
                elapsed_ms = 0,
                error_code = "CAPTURE_THREAD_START_FAILED",
                "failed to spawn capture thread"
            );
        }
    }

    /// Starts the normal translation/display half of the pipeline with text
    /// that the passive selection monitor has already read through UIA.
    pub fn trigger_captured(&self, app: AppHandle, selection: CapturedSelection) -> bool {
        self.trigger_captured_checked(app, selection, None)
    }

    fn trigger_captured_checked(
        &self,
        app: AppHandle,
        selection: CapturedSelection,
        probe: Option<&CaptureRequest>,
    ) -> bool {
        let context = selection.foreground_context.clone();
        if !context.is_valid_and_foreground() {
            return false;
        }
        let request = {
            let _coordination = self
                .coordination
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let latest = app
                .state::<crate::latest_capture_store::LatestCaptureStore>()
                .get();
            let config = app.state::<ModelConfigStore>().get();
            let action = self
                .task()
                .selection(&selection, &config, latest.as_ref(), probe);
            match action {
                SelectionAction::Stale => return false,
                SelectionAction::Reuse => return true,
                SelectionAction::Start(request) => {
                    self.overlay.begin_request(request.id);
                    request
                }
            }
        };

        let started = Instant::now();
        let language_profile = LanguageProfile::analyze(&selection.text);
        let model_config = app.state::<ModelConfigStore>().get();
        let translation_request = TranslationRequest::new(
            selection.text.clone(),
            model_config.mode,
            context.application_name.clone(),
            ContentType::Unknown,
        )
        .with_academic_context(
            selection.context_before.clone(),
            selection.context_after.clone(),
        );
        let anchor = selection.selection_rect.unwrap_or_else(cursor_anchor);
        let mut payload = CapturePayload {
            request_id: request.id,
            phase: CapturePhase::Translating,
            success: false,
            text: String::new(),
            application_name: context.application_name.clone(),
            process_id: context.process_id,
            capture_method: selection.source,
            elapsed_ms: selection.elapsed_ms,
            selection_rect: selection.selection_rect,
            error_code: None,
            error_message: None,
            focus_preserved: true,
            clipboard_restored: selection.clipboard_restored,
            warning_code: selection.warning_code,
            language_profile: Some(language_profile),
            translation_mode: Some(model_config.mode),
            tone_note: None,
        };
        info!(
            application = %context.application_name,
            capture_method = %payload.capture_method,
            text_length = selection.text.chars().count(),
            elapsed_ms = payload.elapsed_ms,
            error_code = "",
            "detected selection accepted from translation button"
        );

        let _coordination = self
            .coordination
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let foreground_unchanged = context.is_valid_and_foreground();
        if !can_commit(self.task().is_current(&request), foreground_unchanged) {
            return false;
        }

        let prepared = PreparedTranslation {
            request: translation_request,
            backend: model_config.backend,
        };
        let mut local_display_gate = None;
        if should_prestart_translation(prepared.backend) {
            let (display_ready, wait_for_display) = tauri::async_runtime::channel(1);
            self.start_translation(
                app.clone(),
                request.clone(),
                payload.clone(),
                prepared.request.clone(),
                started,
                Some(wait_for_display),
            );
            local_display_gate = Some(display_ready);
        }

        match self.overlay.show(&app, &payload, &context, anchor) {
            Ok(focus_preserved) => {
                payload.focus_preserved = focus_preserved;
                if !focus_preserved {
                    let _ = self.overlay.hide(&app);
                    return false;
                }
            }
            Err(error) => {
                error!(
                    application = %context.application_name,
                    capture_method = %payload.capture_method,
                    text_length = 0,
                    elapsed_ms = payload.elapsed_ms,
                    error_code = "OVERLAY_SHOW_FAILED",
                    %error,
                    "selection-button translation overlay failed to show"
                );
                return false;
            }
        }

        if let Some(display_ready) = local_display_gate {
            let _ = display_ready.try_send(());
        } else {
            self.start_translation(app, request, payload, prepared.request, started, None);
        }
        true
    }

    /// Retry the failed source, not whatever happens to be selected now.
    pub fn retry(&self, app: AppHandle, request_id: u64) -> Result<bool, String> {
        let guard = self.coordination.lock().unwrap_or_else(|e| e.into_inner());
        let Some(mut payload) = app
            .state::<crate::latest_capture_store::LatestCaptureStore>()
            .get()
            .filter(|p| p.request_id == request_id)
        else {
            return Ok(false);
        };
        // Capture only the foreground window identity for safe non-activating display.
        let context = if payload.phase == CapturePhase::TranslationFailed {
            Some(ForegroundContext::capture().map_err(|e| e.to_string())?)
        } else {
            None
        };
        let action = self.task().retry(request_id, Some(&payload))?;
        let (request, source) = match action {
            RetryAction::Ignore => return Ok(false),
            RetryAction::Capture(request) => {
                self.overlay.begin_request(request.id);
                drop(guard);
                self.trigger_capture(app, request);
                return Ok(true);
            }
            RetryAction::Translate(request, source) => (request, source),
        };
        let context = context.expect("translation retry has captured foreground identity");
        self.overlay.begin_request(request.id);
        payload.request_id = request.id;
        payload.phase = CapturePhase::Translating;
        payload.success = false;
        payload.text.clear();
        payload.tone_note = None;
        payload.error_code = None;
        payload.error_message = None;
        payload.elapsed_ms = 0;
        let anchor = payload.selection_rect.unwrap_or_else(cursor_anchor);
        if !self.overlay.show_retry(&app, &payload, &context, anchor)? {
            return Ok(false);
        }
        self.start_translation(app, request, payload, source, Instant::now(), None);
        Ok(true)
    }

    // Callers hold coordination through task registration and display commit.
    // The task state guard is released before spawning or touching the window.
    fn start_translation(
        &self,
        app: AppHandle,
        request: CaptureRequest,
        base_payload: CapturePayload,
        translation_request: TranslationRequest,
        started: Instant,
        mut display_ready: Option<tauri::async_runtime::Receiver<()>>,
    ) {
        if let Ok(context) = ForegroundContext::capture() {
            let config = app.state::<ModelConfigStore>().get();
            self.task()
                .started(&request, translation_request.clone(), config, context);
        }
        let coordinator = self.clone();
        tauri::async_runtime::spawn(async move {
            let service = app.state::<TranslationService>().inner().clone();
            let request_started_ms = started.elapsed().as_millis();
            let model_started = Instant::now();
            let result = service.translate(&translation_request).await;
            let model_elapsed_ms = model_started.elapsed().as_millis();
            if !coordinator.task().is_current(&request) {
                return;
            }
            if let Some(receiver) = display_ready.as_mut()
                && receiver.recv().await.is_none()
            {
                info!(
                    application = %base_payload.application_name,
                    capture_method = "model-translation",
                    text_length = 0,
                    request_started_ms,
                    model_elapsed_ms,
                    error_code = "OVERLAY_NOT_READY",
                    "translation result discarded before overlay became ready"
                );
                return;
            }
            if !coordinator.task().is_current(&request) {
                return;
            }

            let _coordination = coordinator
                .coordination
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if !coordinator.task().complete(
                &request,
                result.as_ref().err().map(|_| &translation_request),
            ) {
                return;
            }
            let mut final_payload = base_payload;
            final_payload.elapsed_ms = started.elapsed().as_millis();
            let mut explanation_request = None;
            match result {
                Ok(translation) => {
                    explanation_request = ExplanationRequest::from_translation(
                        request.id,
                        &translation_request,
                        translation.text.clone(),
                    );
                    final_payload.phase = CapturePhase::Translated;
                    final_payload.success = true;
                    final_payload.text = translation.text;
                    final_payload.tone_note = translation.tone_note;
                    final_payload.error_code = None;
                    final_payload.error_message = None;
                    info!(
                        application = %final_payload.application_name,
                        capture_method = "model-translation",
                        text_length = final_payload.text.chars().count(),
                        elapsed_ms = final_payload.elapsed_ms,
                        request_started_ms,
                        model_elapsed_ms,
                        error_code = "",
                        "translation completed"
                    );
                }
                Err(message) => {
                    final_payload.phase = CapturePhase::TranslationFailed;
                    final_payload.success = false;
                    final_payload.text.clear();
                    final_payload.error_code = Some("TRANSLATION_FAILED".to_owned());
                    final_payload.error_message = Some(message);
                    warn!(
                        application = %final_payload.application_name,
                        capture_method = "model-translation",
                        text_length = 0,
                        elapsed_ms = final_payload.elapsed_ms,
                        request_started_ms,
                        model_elapsed_ms,
                        error_code = "TRANSLATION_FAILED",
                        "translation failed"
                    );
                }
            }

            match coordinator
                .overlay
                .update(&app, &final_payload, explanation_request)
            {
                Ok(()) => {}
                Err(_) if coordinator.task().is_current(&request) => {
                    warn!(
                        application = %final_payload.application_name,
                        capture_method = "model-translation",
                        text_length = 0,
                        elapsed_ms = final_payload.elapsed_ms,
                        error_code = "OVERLAY_UPDATE_FAILED",
                        "translation result could not update the overlay"
                    );
                }
                Err(_) => {}
            }
        });
    }
}

fn run_pipeline(
    context: ForegroundContext,
    cancellation: Arc<crate::capture_session::CancellationToken>,
    started: Instant,
    capture_academic_context: bool,
) -> Result<CapturedSelection, PipelineFailure> {
    match SelectionService::capture_with_timeout(
        context.clone(),
        cancellation.clone(),
        UIA_TIMEOUT,
        None,
        capture_academic_context,
    ) {
        Ok(selection) => Ok(CapturedSelection {
            text: selection.text,
            context_before: selection.context_before,
            context_after: selection.context_after,
            source: selection.method.to_owned(),
            selection_rect: selection.rect,
            foreground_context: context,
            elapsed_ms: started.elapsed().as_millis(),
            clipboard_restored: None,
            warning_code: None,
        }),
        Err(uia_failure) if should_try_clipboard(&uia_failure) => {
            if cancellation.is_cancelled() || !context.is_valid_and_foreground() {
                return Err(PipelineFailure {
                    code: "FOREGROUND_CHANGED".to_owned(),
                    message: "前台应用已经改变".to_owned(),
                    source: "UI Automation".to_owned(),
                    clipboard_restored: None,
                    warning_code: None,
                });
            }
            match ClipboardService::capture_with_timeout(context.clone(), cancellation) {
                Ok(capture) => Ok(CapturedSelection {
                    text: capture.text,
                    context_before: None,
                    context_after: None,
                    source: CLIPBOARD_METHOD.to_owned(),
                    selection_rect: None,
                    foreground_context: context,
                    elapsed_ms: started.elapsed().as_millis(),
                    clipboard_restored: Some(capture.restored),
                    warning_code: capture.warning_code,
                }),
                Err(failure) => Err(PipelineFailure {
                    code: if empty_selection_failure(&uia_failure.code, &failure.code) {
                        "NO_TEXT_SELECTED".into()
                    } else {
                        failure.code
                    },
                    message: failure.message,
                    source: CLIPBOARD_METHOD.to_owned(),
                    clipboard_restored: Some(failure.restored),
                    warning_code: failure.warning_code,
                }),
            }
        }
        Err(failure) => Err(PipelineFailure {
            code: failure.code,
            message: failure.message,
            source: failure.method.to_owned(),
            clipboard_restored: None,
            warning_code: None,
        }),
    }
}

fn should_try_clipboard(failure: &SelectionFailure) -> bool {
    failure.recoverable
        && matches!(
            failure.code.split(':').next().unwrap_or_default(),
            "TEXT_PATTERN_UNSUPPORTED"
                | "NO_TEXT_SELECTED"
                | "SELECTION_RECT_UNAVAILABLE"
                | "UIA_TIMEOUT"
                | "UIA_WORKER_DISCONNECTED"
                | "GET_SELECTION"
                | "SELECTION_LENGTH"
                | "SELECTION_RANGE"
                | "GET_TEXT"
                | "READ_BOUNDING_RECTS"
                | "FOCUSED_ELEMENT"
                | "WINDOW_ROOT"
                | "RAW_WALKER"
                | "CREATE_AUTOMATION"
                | "COM_INIT"
                | "WORKER_START_FAILED"
        )
}

fn empty_selection_failure(uia_code: &str, clipboard_code: &str) -> bool {
    clipboard_code == "CLIPBOARD_TEXT_EMPTY"
        || (uia_code == "NO_TEXT_SELECTED" && clipboard_code == "CLIPBOARD_SEQUENCE_UNCHANGED")
}

fn can_commit(is_current: bool, foreground_unchanged: bool) -> bool {
    is_current && foreground_unchanged
}

fn should_prestart_translation(backend: ModelBackend) -> bool {
    backend == ModelBackend::Local
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_selection_hint_does_not_mask_capture_errors() {
        assert!(empty_selection_failure(
            "NO_TEXT_SELECTED",
            "CLIPBOARD_SEQUENCE_UNCHANGED"
        ));
        assert!(empty_selection_failure(
            "TEXT_PATTERN_UNSUPPORTED",
            "CLIPBOARD_TEXT_EMPTY"
        ));
        assert!(!empty_selection_failure(
            "TEXT_PATTERN_UNSUPPORTED",
            "CLIPBOARD_SEQUENCE_UNCHANGED"
        ));
        assert!(!empty_selection_failure(
            "NO_TEXT_SELECTED",
            "CLIPBOARD_TIMEOUT"
        ));
        assert!(!empty_selection_failure(
            "NO_TEXT_SELECTED",
            "FOREGROUND_CHANGED"
        ));
    }

    fn failure(code: &str, recoverable: bool) -> SelectionFailure {
        SelectionFailure {
            code: code.to_owned(),
            message: "test".to_owned(),
            method: "UI Automation",
            recoverable,
        }
    }

    #[test]
    fn clipboard_runs_only_after_eligible_uia_failure() {
        assert!(should_try_clipboard(&failure(
            "TEXT_PATTERN_UNSUPPORTED",
            true
        )));
        assert!(should_try_clipboard(&failure(
            "GET_SELECTION:0x80004005",
            true
        )));
        assert!(!should_try_clipboard(&failure("PROTECTED_INPUT", false)));
    }

    #[test]
    fn foreground_switch_drops_result_before_commit() {
        assert!(can_commit(true, true));
        assert!(!can_commit(true, false));
        assert!(!can_commit(false, true));
    }

    #[test]
    fn only_local_translation_overlaps_overlay_preparation() {
        assert!(should_prestart_translation(ModelBackend::Local));
        assert!(!should_prestart_translation(ModelBackend::Api));
    }
}
