use tauri::Emitter;
use tracing::warn;

use crate::{
    credential_store::CredentialStore,
    model_config::{ModelBackend, ModelConfig, ModelConfigStore, ModelConfigView, validate_config},
};

use super::{commands::model_config_view, tray};

pub(super) fn alternate_model_backend(backend: ModelBackend) -> ModelBackend {
    match backend {
        ModelBackend::Local => ModelBackend::Api,
        ModelBackend::Api => ModelBackend::Local,
    }
}

pub(super) fn tray_model_switch_label(active: ModelBackend) -> &'static str {
    match active {
        ModelBackend::Local => "切换至 API",
        ModelBackend::Api => "切换至本地模型",
    }
}

fn ensure_model_backend_available(
    config: &ModelConfig,
    credentials: &CredentialStore,
    backend: ModelBackend,
) -> Result<(), String> {
    let mut candidate = config.clone();
    candidate.backend = backend;
    validate_config(&candidate)?;
    if backend == ModelBackend::Api && credentials.api_key_hint(&candidate.api.base_url)?.is_none()
    {
        return Err("请先保存 API Key，再切换到 API".to_owned());
    }
    Ok(())
}

pub(super) fn model_backend_is_available(
    config: &ModelConfig,
    credentials: &CredentialStore,
    backend: ModelBackend,
) -> bool {
    ensure_model_backend_available(config, credentials, backend).is_ok()
}

pub(super) fn switch_model_backend(
    config: &ModelConfigStore,
    credentials: &CredentialStore,
    backend: ModelBackend,
) -> Result<ModelConfig, String> {
    let mut next = config.get();
    if next.backend == backend {
        return Ok(next);
    }
    ensure_model_backend_available(&next, credentials, backend)?;
    next.backend = backend;
    config.save(next.clone())?;
    Ok(next)
}

pub(super) fn select_model_backend(
    config: &ModelConfigStore,
    backend: ModelBackend,
) -> Result<ModelConfig, String> {
    config.select_backend(backend)
}

pub(super) fn refresh_tray_model_switch(
    app: &tauri::AppHandle,
    config: &ModelConfigStore,
    credentials: &CredentialStore,
) {
    if let Err(error) = tray::refresh_model_switch(app, &config.get(), credentials) {
        warn!(
            application = "Translay",
            capture_method = "tray-model-switch",
            text_length = 0,
            elapsed_ms = 0,
            error_code = "TRAY_MODEL_SWITCH_REFRESH_FAILED",
            %error,
            "tray model switch could not be refreshed"
        );
    }
}

pub(super) fn sync_model_backend_surfaces(
    app: &tauri::AppHandle,
    config: &ModelConfigStore,
    credentials: &CredentialStore,
) -> Result<ModelConfigView, String> {
    let view = model_config_view(config, credentials)?;
    refresh_tray_model_switch(app, config, credentials);
    if let Err(error) = app.emit("model-backend-changed", view.clone()) {
        warn!(
            application = "Translay",
            capture_method = "model-backend-sync",
            text_length = 0,
            elapsed_ms = 0,
            error_code = "MODEL_BACKEND_CHANGED_EMIT_FAILED",
            %error,
            "settings window could not be notified about the model switch"
        );
    }
    Ok(view)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switch_target_is_always_the_inactive_backend() {
        assert_eq!(
            alternate_model_backend(ModelBackend::Local),
            ModelBackend::Api
        );
        assert_eq!(
            alternate_model_backend(ModelBackend::Api),
            ModelBackend::Local
        );
    }

    #[test]
    fn menu_label_describes_the_click_result() {
        assert_eq!(tray_model_switch_label(ModelBackend::Local), "切换至 API");
        assert_eq!(tray_model_switch_label(ModelBackend::Api), "切换至本地模型");
    }
}
