use crate::{
    credentials::Credentials,
    model_config::{
        ApiKeyStatus, ModelBackend, ModelConfig, ModelConfigStore, ModelConfigView,
        SaveModelConfig, remember_api_model, remember_local_model, validate_config,
    },
    translation::TranslationMode,
};

pub fn api_key_status(
    credentials: &impl Credentials,
    base_url: &str,
) -> Result<ApiKeyStatus, String> {
    let api_key_hint = credentials.api_key_hint(base_url)?;
    Ok(ApiKeyStatus {
        has_api_key: api_key_hint.is_some(),
        api_key_hint,
    })
}

pub fn model_config_view(
    store: &ModelConfigStore,
    credentials: &impl Credentials,
) -> Result<ModelConfigView, String> {
    let config = store.get();
    let status = api_key_status(credentials, &config.api.base_url)?;
    Ok(ModelConfigView {
        config,
        has_api_key: status.has_api_key,
        api_key_hint: status.api_key_hint,
    })
}

// Preserve the established Windows ordering and validation. Native callers
// refresh their own menus after this operation, before querying the view.
pub fn save_provider_config(
    store: &ModelConfigStore,
    credentials: &impl Credentials,
    input: SaveModelConfig,
) -> Result<(), String> {
    let provider = input.backend;
    let mut next = store.get();
    match provider {
        ModelBackend::Local => {
            next.local = input.local;
            remember_local_model(&mut next);
        }
        ModelBackend::Api => {
            next.api = input.api;
            remember_api_model(&mut next);
        }
    }
    next.timeout_seconds = input.timeout_seconds;
    let mut validation_candidate = next.clone();
    validation_candidate.backend = provider;
    validate_config(&validation_candidate)?;
    let api_base_url = next.api.base_url.clone();
    store.save(next)?;
    if provider == ModelBackend::Api
        && let Some(api_key) = input.api_key.filter(|value| !value.trim().is_empty())
    {
        credentials.write_api_key(&api_base_url, &api_key)?;
    }
    Ok(())
}

pub fn save_translation_preferences(
    store: &ModelConfigStore,
    mode: TranslationMode,
    reasoning_enabled: bool,
) -> Result<(), String> {
    let mut next = store.get();
    next.mode = mode;
    next.reasoning_enabled = reasoning_enabled;
    store.save(next)
}

pub fn connection_config(input: &SaveModelConfig) -> ModelConfig {
    ModelConfig {
        backend: input.backend,
        mode: input.mode,
        selection_icon_enabled: true,
        reasoning_enabled: input.reasoning_enabled,
        local: input.local.clone(),
        api: input.api.clone(),
        local_models: Default::default(),
        api_models: Default::default(),
        timeout_seconds: input.timeout_seconds,
    }
}
