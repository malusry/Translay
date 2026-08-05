use std::{
    sync::{Arc, Mutex},
    thread,
    time::Instant,
};

use tauri::{AppHandle, Manager};
use tracing::{error, info, warn};

use crate::{
    capture_session::{CaptureRequest, CaptureSession},
    clipboard_service::{CLIPBOARD_METHOD, ClipboardService},
    config::UIA_TIMEOUT,
    foreground_context::ForegroundContext,
    model_config::{ModelBackend, ModelConfigStore},
    models::{CapturePayload, CapturePhase, CapturedSelection},
    overlay_manager::{OverlayManager, cursor_anchor},
    selection_service::{SelectionFailure, SelectionService},
    translation::{ContentType, LanguageProfile, TranslationMode, TranslationRequest},
    translation_service::TranslationService,
};

#[derive(Clone)]
pub struct CaptureCoordinator {
    sessions: Arc<CaptureSession>,
    overlay: OverlayManager,
    coordination: Arc<Mutex<()>>,
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
            sessions: Arc::new(CaptureSession::default()),
            overlay,
            coordination: Arc::new(Mutex::new(())),
        }
    }

    pub fn overlay(&self) -> &OverlayManager {
        &self.overlay
    }

    pub fn cancel_current(&self) {
        let _coordination = self
            .coordination
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        self.sessions.cancel_current();
    }

    pub fn trigger(&self, app: AppHandle) {
        // Every press starts a new capture. CaptureSession cancels the previous
        // generation, while the currently visible overlay remains until it is
        // replaced or auto-hidden.
        let request = {
            let _coordination = self
                .coordination
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            self.sessions.begin()
        };
        self.overlay.begin_request(request.id);
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
        };
        {
            let _coordination = self
                .coordination
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if !can_commit(
                self.sessions.is_current(&request),
                context.is_valid_and_foreground(),
            ) {
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
                if !coordinator.sessions.is_current(&request) {
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
                                error_code: Some(failure.code),
                                error_message: Some(failure.message),
                                focus_preserved: true,
                                clipboard_restored: failure.clipboard_restored,
                                warning_code: failure.warning_code,
                                language_profile: None,
                                translation_mode: None,
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
                if !can_commit(
                    coordinator.sessions.is_current(&request),
                    context.is_valid_and_foreground(),
                ) {
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
        let context = selection.foreground_context.clone();
        if !context.is_valid_and_foreground() {
            return false;
        }
        let request = {
            let _coordination = self
                .coordination
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            self.sessions.begin()
        };
        self.overlay.begin_request(request.id);

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
        if !can_commit(
            self.sessions.is_current(&request),
            context.is_valid_and_foreground(),
        ) {
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

    fn start_translation(
        &self,
        app: AppHandle,
        request: CaptureRequest,
        base_payload: CapturePayload,
        translation_request: TranslationRequest,
        started: Instant,
        mut display_ready: Option<tauri::async_runtime::Receiver<()>>,
    ) {
        let coordinator = self.clone();
        tauri::async_runtime::spawn(async move {
            let service = app.state::<TranslationService>().inner().clone();
            let request_started_ms = started.elapsed().as_millis();
            let model_started = Instant::now();
            let result = service.translate(&translation_request).await;
            let model_elapsed_ms = model_started.elapsed().as_millis();
            if !coordinator.sessions.is_current(&request) {
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
            if !coordinator.sessions.is_current(&request) {
                return;
            }

            let mut final_payload = base_payload;
            final_payload.elapsed_ms = started.elapsed().as_millis();
            match result {
                Ok(translation) => {
                    final_payload.phase = CapturePhase::Translated;
                    final_payload.success = true;
                    final_payload.text = translation;
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

            if coordinator.overlay.update(&app, &final_payload).is_err()
                && coordinator.sessions.is_current(&request)
            {
                warn!(
                    application = %final_payload.application_name,
                    capture_method = "model-translation",
                    text_length = 0,
                    elapsed_ms = final_payload.elapsed_ms,
                    error_code = "OVERLAY_UPDATE_FAILED",
                    "translation result could not update the overlay"
                );
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
                    code: failure.code,
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

fn can_commit(is_current: bool, foreground_unchanged: bool) -> bool {
    is_current && foreground_unchanged
}

fn should_prestart_translation(backend: ModelBackend) -> bool {
    backend == ModelBackend::Local
}

#[cfg(test)]
mod tests {
    use super::*;

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
