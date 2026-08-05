use std::sync::atomic::{AtomicBool, Ordering};

use tracing::info;

use crate::{
    capture_coordinator::CaptureCoordinator,
    capture_session::CaptureSession,
    clipboard_service::{CLIPBOARD_METHOD, ClipboardService},
    credential_store::CredentialStore,
    latest_capture_store::LatestCaptureStore,
    model_config::{
        ApiKeyStatus, ModelBackend, ModelConfig, ModelConfigStore, ModelConfigView,
        SaveModelConfig, validate_config,
    },
    models,
    overlay_manager::OverlayManager,
    selection_button::SelectionButtonManager,
    translation::TranslationMode,
    translation_service::{ConnectionTestResult, TranslationService},
};

use super::{model_backend, windows};

static OVERLAY_FRONTEND_READY: AtomicBool = AtomicBool::new(false);

#[tauri::command]
pub(super) fn overlay_frontend_ready() {
    if !OVERLAY_FRONTEND_READY.swap(true, Ordering::AcqRel) {
        info!(
            application = "Translay",
            capture_method = "overlay-ipc",
            text_length = 0,
            elapsed_ms = 0,
            error_code = "",
            "overlay frontend IPC is ready"
        );
    }
}

#[tauri::command]
pub(super) fn get_latest_capture(
    store: tauri::State<'_, LatestCaptureStore>,
) -> Option<models::CapturePayload> {
    store.get()
}

#[tauri::command]
pub(super) fn ack_capture(
    app: tauri::AppHandle,
    overlay: tauri::State<'_, OverlayManager>,
    request_id: u64,
) -> bool {
    overlay.acknowledge(&app, request_id)
}

#[tauri::command]
pub(super) fn dismiss_overlay(
    app: tauri::AppHandle,
    overlay: tauri::State<'_, OverlayManager>,
    request_id: u64,
) -> bool {
    overlay.dismiss_request(&app, request_id)
}

#[tauri::command]
pub(super) fn set_overlay_hovered(
    overlay: tauri::State<'_, OverlayManager>,
    request_id: u64,
    hovered: bool,
) -> bool {
    overlay.set_hovered(request_id, hovered)
}

#[tauri::command]
pub(super) fn fit_overlay_height(
    app: tauri::AppHandle,
    overlay: tauri::State<'_, OverlayManager>,
    request_id: u64,
    logical_height: i32,
) -> bool {
    overlay.fit_height(&app, request_id, logical_height)
}

#[tauri::command]
pub(super) fn retry_capture(
    app: tauri::AppHandle,
    coordinator: tauri::State<'_, CaptureCoordinator>,
) {
    coordinator.trigger(app);
}

#[tauri::command]
pub(super) fn translate_detected_selection(
    app: tauri::AppHandle,
    coordinator: tauri::State<'_, CaptureCoordinator>,
    selection_button: tauri::State<'_, SelectionButtonManager>,
) -> bool {
    let Some(selection) = selection_button.take_candidate(&app) else {
        return false;
    };
    let selection = validate_detected_selection_with_clipboard(selection);
    coordinator.trigger_captured(app, selection)
}

fn validate_detected_selection_with_clipboard(
    mut selection: models::CapturedSelection,
) -> models::CapturedSelection {
    let validation_session = CaptureSession::default();
    let request = validation_session.begin();
    if let Ok(capture) = ClipboardService::capture_with_timeout(
        selection.foreground_context.clone(),
        request.cancellation,
    ) {
        selection.text = capture.text;
        selection.source = format!("{CLIPBOARD_METHOD}:SelectionValidation");
        selection.clipboard_restored = Some(capture.restored);
        selection.warning_code = capture.warning_code;
    }
    selection
}

#[tauri::command]
pub(super) fn copy_translation(text: String) -> Result<(), String> {
    ClipboardService::write_text(&text)
}

#[tauri::command]
pub(super) fn get_model_config(
    config: tauri::State<'_, ModelConfigStore>,
    credentials: tauri::State<'_, CredentialStore>,
) -> Result<ModelConfigView, String> {
    model_config_view(config.inner(), credentials.inner())
}

pub(super) fn model_config_view(
    config: &ModelConfigStore,
    credentials: &CredentialStore,
) -> Result<ModelConfigView, String> {
    let config = config.get();
    let status = read_api_key_status(credentials, &config.api.base_url)?;
    Ok(ModelConfigView {
        config,
        has_api_key: status.has_api_key,
        api_key_hint: status.api_key_hint,
    })
}

#[tauri::command]
pub(super) fn get_model_api_key_status(
    base_url: String,
    credentials: tauri::State<'_, CredentialStore>,
) -> Result<ApiKeyStatus, String> {
    read_api_key_status(credentials.inner(), &base_url)
}

fn read_api_key_status(
    credentials: &CredentialStore,
    base_url: &str,
) -> Result<ApiKeyStatus, String> {
    let api_key_hint = credentials.api_key_hint(base_url)?;
    Ok(ApiKeyStatus {
        has_api_key: api_key_hint.is_some(),
        api_key_hint,
    })
}

#[tauri::command]
pub(super) fn save_model_provider_config(
    app: tauri::AppHandle,
    input: SaveModelConfig,
    config: tauri::State<'_, ModelConfigStore>,
    credentials: tauri::State<'_, CredentialStore>,
) -> Result<ModelConfigView, String> {
    let provider = input.backend;
    let mut next = config.get();
    match provider {
        ModelBackend::Local => next.local = input.local,
        ModelBackend::Api => next.api = input.api,
    }
    next.timeout_seconds = input.timeout_seconds;

    // Validate the provider being edited without changing the active provider.
    let mut validation_candidate = next.clone();
    validation_candidate.backend = provider;
    validate_config(&validation_candidate)?;
    let api_base_url = next.api.base_url.clone();
    config.save(next)?;
    if provider == ModelBackend::Api
        && let Some(api_key) = input.api_key.filter(|value| !value.trim().is_empty())
    {
        credentials.write_api_key(&api_base_url, &api_key)?;
    }
    model_backend::refresh_tray_model_switch(&app, config.inner(), credentials.inner());
    model_config_view(config.inner(), credentials.inner())
}

#[tauri::command]
pub(super) fn activate_model_backend(
    app: tauri::AppHandle,
    backend: ModelBackend,
    config: tauri::State<'_, ModelConfigStore>,
    credentials: tauri::State<'_, CredentialStore>,
) -> Result<ModelConfigView, String> {
    model_backend::switch_model_backend(config.inner(), credentials.inner(), backend)?;
    model_backend::sync_model_backend_surfaces(&app, config.inner(), credentials.inner())
}

#[tauri::command]
pub(super) fn save_translation_preferences(
    mode: TranslationMode,
    reasoning_enabled: bool,
    config: tauri::State<'_, ModelConfigStore>,
) -> Result<(), String> {
    let mut next = config.get();
    next.mode = mode;
    next.reasoning_enabled = reasoning_enabled;
    config.save(next)
}

#[tauri::command]
pub(super) fn clear_model_api_key(
    app: tauri::AppHandle,
    base_url: String,
    config: tauri::State<'_, ModelConfigStore>,
    credentials: tauri::State<'_, CredentialStore>,
) -> Result<ApiKeyStatus, String> {
    credentials.clear_api_key(&base_url)?;
    model_backend::refresh_tray_model_switch(&app, config.inner(), credentials.inner());
    read_api_key_status(credentials.inner(), &base_url)
}

#[tauri::command]
pub(super) async fn test_model_connection(
    input: SaveModelConfig,
    service: tauri::State<'_, TranslationService>,
) -> Result<ConnectionTestResult, String> {
    let config = ModelConfig {
        backend: input.backend,
        mode: input.mode,
        reasoning_enabled: input.reasoning_enabled,
        local: input.local,
        api: input.api,
        timeout_seconds: input.timeout_seconds,
    };
    let service = service.inner().clone();
    Ok(service
        .test_connection_with_config(config, input.api_key)
        .await)
}

#[tauri::command]
pub(super) fn start_settings_dragging(app: tauri::AppHandle) -> Result<(), String> {
    windows::start_settings_dragging(&app)
}

#[tauri::command]
pub(super) fn hide_settings_window(app: tauri::AppHandle) -> Result<(), String> {
    windows::hide_settings_window(&app)
}

#[tauri::command]
pub(super) fn minimize_settings_window(app: tauri::AppHandle) -> Result<(), String> {
    windows::minimize_settings_window(&app)
}
