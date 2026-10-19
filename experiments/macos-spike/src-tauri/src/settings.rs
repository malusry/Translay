use crate::{
    credentials::CredentialStore,
    desktop,
    state::{Action, Snapshot},
};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};
use translay_core::{
    credentials::Credentials,
    model_config::{
        ApiKeyStatus, ModelBackend, ModelConfigStore, ModelConfigView, SaveModelConfig,
    },
    model_transport::{ConnectionTestResult, ModelClient},
    settings as shared,
    translation::TranslationMode,
};

#[derive(Default)]
pub struct SettingsWrites(pub Mutex<()>);

pub fn ensure_running(app: &AppHandle) -> Result<(), String> {
    if desktop::snapshot(app).exiting {
        Err("程序正在退出".into())
    } else {
        Ok(())
    }
}

pub fn current_view(app: &AppHandle) -> Result<ModelConfigView, String> {
    shared::model_config_view(
        app.state::<ModelConfigStore>().inner(),
        app.state::<CredentialStore>().inner(),
    )
}

// Call with SettingsWrites held. Keep native controls on the persisted state.
pub fn synchronize(app: &AppHandle) -> Result<ModelConfigView, String> {
    let config = app.state::<ModelConfigStore>().get();
    {
        let state = app.state::<Mutex<Snapshot>>();
        let mut state = state.lock().unwrap_or_else(|error| error.into_inner());
        state.apply(Action::SelectMode(config.mode));
        state.apply(Action::SetSelectionIcon(config.selection_icon_enabled));
    }
    desktop::publish(app).map_err(|error| error.to_string())?;
    let view = current_view(app)?;
    app.emit("model-backend-changed", &view)
        .map_err(|error| error.to_string())?;
    Ok(view)
}

#[tauri::command]
pub fn get_model_config(app: AppHandle) -> Result<ModelConfigView, String> {
    current_view(&app)
}

#[tauri::command]
pub fn get_model_api_key_status(
    base_url: String,
    credentials: tauri::State<'_, CredentialStore>,
) -> Result<ApiKeyStatus, String> {
    shared::api_key_status(credentials.inner(), &base_url)
}

#[tauri::command]
pub fn save_model_provider_config(
    app: AppHandle,
    input: SaveModelConfig,
) -> Result<ModelConfigView, String> {
    let gate = app.state::<SettingsWrites>();
    let _guard = gate.0.lock().unwrap_or_else(|error| error.into_inner());
    ensure_running(&app)?;
    shared::save_provider_config(
        app.state::<ModelConfigStore>().inner(),
        app.state::<CredentialStore>().inner(),
        input,
    )?;
    synchronize(&app)
}

#[tauri::command]
pub fn activate_model_backend(
    app: AppHandle,
    backend: ModelBackend,
) -> Result<ModelConfigView, String> {
    let gate = app.state::<SettingsWrites>();
    let _guard = gate.0.lock().unwrap_or_else(|error| error.into_inner());
    ensure_running(&app)?;
    app.state::<ModelConfigStore>().select_backend(backend)?;
    synchronize(&app)
}

#[tauri::command]
pub fn save_translation_preferences(
    app: AppHandle,
    mode: TranslationMode,
    reasoning_enabled: bool,
) -> Result<(), String> {
    let gate = app.state::<SettingsWrites>();
    let _guard = gate.0.lock().unwrap_or_else(|error| error.into_inner());
    ensure_running(&app)?;
    shared::save_translation_preferences(
        app.state::<ModelConfigStore>().inner(),
        mode,
        reasoning_enabled,
    )?;
    synchronize(&app)?;
    Ok(())
}

#[tauri::command]
pub fn clear_model_api_key(app: AppHandle, base_url: String) -> Result<ApiKeyStatus, String> {
    let gate = app.state::<SettingsWrites>();
    let _guard = gate.0.lock().unwrap_or_else(|error| error.into_inner());
    ensure_running(&app)?;
    let credentials = app.state::<CredentialStore>();
    credentials.clear_api_key(&base_url)?;
    synchronize(&app)?;
    shared::api_key_status(credentials.inner(), &base_url)
}

#[tauri::command]
pub async fn test_model_connection(
    app: AppHandle,
    input: SaveModelConfig,
) -> Result<ConnectionTestResult, String> {
    ensure_running(&app)?;
    let config = shared::connection_config(&input);
    translay_core::model_config::validate_config(&config)?;
    let api_key = if config.backend == ModelBackend::Api {
        input.api_key.filter(|key| !key.trim().is_empty()).or(app
            .state::<CredentialStore>()
            .read_api_key(&config.api.base_url)?)
    } else {
        None
    };
    let client = app.state::<ModelClient>().inner().clone();
    Ok(client.test_connection_with_config(config, api_key).await)
}

#[tauri::command]
pub async fn detect_active_local_model(provider: String, base_url: String) -> Option<String> {
    translay_core::local_model_detection::detect(&provider, &base_url).await
}

#[tauri::command]
pub fn open_provider_api_portal(provider: String) -> Result<(), String> {
    let url = translay_core::portals::provider_api_portal_url(&provider)
        .ok_or_else(|| "未知的模型服务，无法打开官方页面".to_owned())?;
    let status = std::process::Command::new("/usr/bin/open")
        .arg(url)
        .status()
        .map_err(|_| "无法调用默认浏览器打开模型服务官方页面".to_owned())?;
    if status.success() {
        Ok(())
    } else {
        Err("模型服务官方页面未能打开".into())
    }
}

#[tauri::command]
pub fn hide_settings_window(app: AppHandle) -> Result<(), String> {
    app.get_webview_window("settings")
        .ok_or("配置窗口不存在")?
        .hide()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn minimize_settings_window(app: AppHandle) -> Result<(), String> {
    app.get_webview_window("settings")
        .ok_or("配置窗口不存在")?
        .minimize()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn start_settings_dragging(app: AppHandle) -> Result<(), String> {
    app.get_webview_window("settings")
        .ok_or("配置窗口不存在")?
        .start_dragging()
        .map_err(|error| error.to_string())
}
