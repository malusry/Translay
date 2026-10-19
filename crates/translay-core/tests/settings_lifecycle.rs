use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use translay_core::{
    credentials::{Credentials, api_key_target},
    model_config::{ModelBackend, ModelConfig, ModelConfigStore, SaveModelConfig},
    settings,
    translation::TranslationMode,
};

#[derive(Clone, Default)]
struct MemoryCredentials(Arc<Mutex<BTreeMap<String, String>>>);
impl Credentials for MemoryCredentials {
    fn write_api_key(&self, base_url: &str, api_key: &str) -> Result<(), String> {
        self.0
            .lock()
            .unwrap()
            .insert(api_key_target(base_url)?, api_key.trim().to_owned());
        Ok(())
    }
    fn read_api_key(&self, base_url: &str) -> Result<Option<String>, String> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .get(&api_key_target(base_url)?)
            .cloned())
    }
    fn clear_api_key(&self, base_url: &str) -> Result<(), String> {
        self.0.lock().unwrap().remove(&api_key_target(base_url)?);
        Ok(())
    }
}
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("translay-core-{}-{stamp}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> PathBuf {
        self.0.join("model-config.json")
    }
    fn store(&self) -> ModelConfigStore {
        ModelConfigStore::load_from_path(self.path()).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn input(config: ModelConfig) -> SaveModelConfig {
    SaveModelConfig {
        backend: config.backend,
        mode: config.mode,
        reasoning_enabled: config.reasoning_enabled,
        local: config.local,
        api: config.api,
        timeout_seconds: config.timeout_seconds,
        api_key: None,
    }
}

#[test]
fn configuration_survives_restart_without_serializing_secrets_or_stale_preferences() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let credentials = MemoryCredentials::default();
    assert!(!fixture.path().exists());
    let mut local = input(store.get());
    local.local.model = "test-local-model".into();
    settings::save_provider_config(&store, &credentials, local).unwrap();
    let mut draft = input(store.get());
    store.set_selection_icon_enabled(false).unwrap();
    settings::save_translation_preferences(&store, TranslationMode::Academic, true).unwrap();
    draft.backend = ModelBackend::Api;
    draft.api.model = "test-api-model".into();
    draft.timeout_seconds = 90;
    let fake_key = ["fake-fixture", "-secret-7F2A"].concat();
    draft.api_key = Some(fake_key.clone());
    settings::save_provider_config(&store, &credentials, draft).unwrap();
    assert_eq!(
        store.get().backend,
        ModelBackend::Local,
        "editing a provider must not activate it"
    );
    store.select_backend(ModelBackend::Api).unwrap();
    let view = settings::model_config_view(&store, &credentials).unwrap();
    assert!(view.has_api_key);
    assert_eq!(view.api_key_hint.as_deref(), Some("••••••••7F2A"));
    assert!(!serde_json::to_string(&view).unwrap().contains(&fake_key));
    assert!(
        !fs::read_to_string(fixture.path())
            .unwrap()
            .contains(&fake_key)
    );
    assert!(!store.get().selection_icon_enabled);
    let restored = fixture.store().get();
    assert!(
        restored.selection_icon_enabled,
        "existing startup-enable behavior is preserved"
    );
    assert_eq!(restored.backend, ModelBackend::Api);
    assert_eq!(restored.mode, TranslationMode::Academic);
    assert!(restored.reasoning_enabled);
    assert_eq!(restored.timeout_seconds, 90);
    assert_eq!(
        restored.api_models.get("openai").map(String::as_str),
        Some("test-api-model")
    );
}

#[test]
fn rejected_configuration_does_not_write_files_or_credentials() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let credentials = MemoryCredentials::default();
    let original = store.get();
    let mut draft = input(original.clone());
    draft.backend = ModelBackend::Api;
    draft.api.base_url = "http://remote.invalid/v1".into();
    draft.api.model = "test".into();
    draft.api_key = Some("fake-fixture-secret".into());
    assert!(settings::save_provider_config(&store, &credentials, draft).is_err());
    assert_eq!(store.get(), original);
    assert!(!fixture.path().exists());
    assert!(credentials.0.lock().unwrap().is_empty());
}

#[test]
fn keys_are_scoped_by_provider_and_deleting_one_preserves_the_other() {
    let credentials = MemoryCredentials::default();
    credentials
        .write_api_key("https://first.invalid/v1", "fake-first")
        .unwrap();
    credentials
        .write_api_key("https://second.invalid/v1", "fake-second")
        .unwrap();
    assert_eq!(
        credentials
            .read_api_key("https://first.invalid/chat/completions")
            .unwrap()
            .as_deref(),
        Some("fake-first")
    );
    credentials
        .clear_api_key("https://first.invalid/v1")
        .unwrap();
    assert!(
        !settings::api_key_status(&credentials, "https://first.invalid/v1")
            .unwrap()
            .has_api_key
    );
    assert!(
        settings::api_key_status(&credentials, "https://second.invalid/v1")
            .unwrap()
            .has_api_key
    );
}

#[test]
fn menu_mode_can_be_saved_before_model_setup_and_failed_save_keeps_state() {
    let fixture = Fixture::new();
    let store = fixture.store();
    store.set_mode(TranslationMode::Academic).unwrap();
    assert_eq!(fixture.store().get().mode, TranslationMode::Academic);
    fs::remove_file(fixture.path()).unwrap();
    fs::create_dir(fixture.path()).unwrap();
    assert!(store.set_mode(TranslationMode::Conversational).is_err());
    assert_eq!(store.get().mode, TranslationMode::Academic);
}
