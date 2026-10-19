use std::sync::atomic::{AtomicBool, Ordering};

use tracing::info;

use crate::{
    capture_coordinator::CaptureCoordinator,
    capture_session::CaptureSession,
    clipboard_service::{CLIPBOARD_METHOD, ClipboardService},
    credential_store::CredentialStore,
    explanation::{ExplanationContent, until_cancelled},
    latest_capture_store::LatestCaptureStore,
    model_config::{
        ApiKeyStatus, ModelBackend, ModelConfigStore, ModelConfigView, SaveModelConfig,
    },
    models,
    overlay_manager::OverlayManager,
    selection_button::SelectionButtonManager,
    translation::TranslationMode,
    translation_service::{ConnectionTestResult, TranslationService},
};

use super::{local_model_detection, model_backend, windows};

static OVERLAY_FRONTEND_READY: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
use translay_core::portals::PROVIDER_API_PORTALS;
use translay_core::portals::provider_api_portal_url;

#[tauri::command]
pub(super) fn open_provider_api_portal(provider: String) -> Result<(), String> {
    let url = provider_api_portal_url(&provider)
        .ok_or_else(|| "未知的模型服务，无法打开官方页面".to_owned())?;
    open_external_url(url)
}

fn open_external_url(url: &str) -> Result<(), String> {
    use ::windows::{
        Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
        core::PCWSTR,
    };

    let operation = "open\0".encode_utf16().collect::<Vec<_>>();
    let target = url
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(operation.as_ptr()),
            PCWSTR(target.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    if result.0 as isize <= 32 {
        return Err("无法调用默认浏览器打开模型服务官方页面".to_owned());
    }
    Ok(())
}

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
    preserve_position: bool,
) -> bool {
    overlay.fit_height(&app, request_id, logical_height, preserve_position)
}

#[tauri::command]
pub(super) fn retry_capture(
    app: tauri::AppHandle,
    request_id: u64,
    coordinator: tauri::State<'_, CaptureCoordinator>,
) -> Result<bool, String> {
    coordinator.retry(app, request_id)
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
pub(super) async fn explain_translation(
    request_id: u64,
    attempt_id: u64,
    store: tauri::State<'_, LatestCaptureStore>,
    service: tauri::State<'_, TranslationService>,
) -> Result<Option<ExplanationContent>, String> {
    let store = store.inner().clone();
    if let Some(cached) = store.cached_explanation(request_id) {
        return Ok(Some(cached));
    }
    let Some((request, cancelled)) = store.begin_explanation(request_id, attempt_id) else {
        return Ok(None);
    };
    let service = service.inner().clone();
    let Some(result) = until_cancelled(cancelled, service.explain(&request)).await else {
        return Ok(None);
    };
    // Dropping the losing HTTP future stops client-side waiting and reading.
    if !store.finish_explanation(request_id, attempt_id, result.as_ref().ok().cloned()) {
        return Ok(None);
    }
    result.map(Some)
}

#[tauri::command]
pub(super) fn cancel_explanation(
    request_id: u64,
    attempt_id: u64,
    store: tauri::State<'_, LatestCaptureStore>,
) -> bool {
    store.cancel_explanation(request_id, attempt_id)
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
    translay_core::settings::model_config_view(config, credentials)
}

#[tauri::command]
pub(super) fn get_model_api_key_status(
    base_url: String,
    credentials: tauri::State<'_, CredentialStore>,
) -> Result<ApiKeyStatus, String> {
    read_api_key_status(credentials.inner(), &base_url)
}

#[tauri::command]
pub(super) async fn detect_active_local_model(
    provider: String,
    base_url: String,
) -> Option<String> {
    local_model_detection::detect(&provider, &base_url).await
}

fn read_api_key_status(
    credentials: &CredentialStore,
    base_url: &str,
) -> Result<ApiKeyStatus, String> {
    translay_core::settings::api_key_status(credentials, base_url)
}

#[tauri::command]
pub(super) fn save_model_provider_config(
    app: tauri::AppHandle,
    input: SaveModelConfig,
    config: tauri::State<'_, ModelConfigStore>,
    credentials: tauri::State<'_, CredentialStore>,
) -> Result<ModelConfigView, String> {
    translay_core::settings::save_provider_config(config.inner(), credentials.inner(), input)?;
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
    model_backend::select_model_backend(config.inner(), backend)?;
    model_backend::sync_model_backend_surfaces(&app, config.inner(), credentials.inner())
}

#[tauri::command]
pub(super) fn save_translation_preferences(
    app: tauri::AppHandle,
    mode: TranslationMode,
    reasoning_enabled: bool,
    config: tauri::State<'_, ModelConfigStore>,
    credentials: tauri::State<'_, CredentialStore>,
) -> Result<(), String> {
    translay_core::settings::save_translation_preferences(config.inner(), mode, reasoning_enabled)?;
    model_backend::refresh_tray_model_switch(&app, config.inner(), credentials.inner());
    Ok(())
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
    let config = translay_core::settings::connection_config(&input);
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

#[cfg(test)]
mod provider_api_portal_tests {
    use super::{PROVIDER_API_PORTALS, provider_api_portal_url};

    #[test]
    fn exposes_the_confirmed_api_and_local_provider_portals() {
        assert_eq!(PROVIDER_API_PORTALS.len(), 11);
        assert_eq!(
            provider_api_portal_url("gemini"),
            Some("https://aistudio.google.com/apikey")
        );
        assert_eq!(
            provider_api_portal_url("ollama"),
            Some("https://docs.ollama.com/api/openai-compatibility")
        );
        assert_eq!(provider_api_portal_url("custom"), None);
        assert_eq!(provider_api_portal_url("https://example.com"), None);
    }
}
