use tauri::{
    Manager,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
use tracing::warn;

use crate::{
    credential_store::CredentialStore,
    model_config::{ModelBackend, ModelConfig, ModelConfigStore},
    translation::TranslationMode,
};

use super::{
    model_backend::{
        alternate_model_backend, model_backend_is_available, switch_model_backend,
        sync_model_backend_surfaces, tray_model_switch_label,
    },
    shutdown::ShutdownCoordinator,
    tray_feedback::TrayFeedbackManager,
    windows::show_settings_window,
};

const MAIN_TRAY_ID: &str = "main-tray";

#[derive(Clone)]
struct TrayModelSwitch {
    item: MenuItem<tauri::Wry>,
}

impl TrayModelSwitch {
    fn refresh(&self, config: &ModelConfig, credentials: &CredentialStore) -> Result<(), String> {
        let target = alternate_model_backend(config.backend);
        self.item
            .set_text(tray_model_switch_label(config.backend))
            .map_err(|error| format!("更新托盘模型切换文案失败：{error}"))?;
        self.item
            .set_enabled(model_backend_is_available(config, credentials, target))
            .map_err(|error| format!("更新托盘模型切换状态失败：{error}"))
    }
}

pub(super) fn refresh_model_switch(
    app: &tauri::AppHandle,
    config: &ModelConfig,
    credentials: &CredentialStore,
) -> Result<(), String> {
    let Some(tray_switch) = app.try_state::<TrayModelSwitch>() else {
        return Ok(());
    };
    tray_switch.refresh(config, credentials)?;
    app.tray_by_id(MAIN_TRAY_ID)
        .ok_or_else(|| "找不到系统托盘图标".to_owned())?
        .set_tooltip(Some(tray_tooltip(config)))
        .map_err(|error| format!("更新托盘提示信息失败：{error}"))
}

fn tray_tooltip(config: &ModelConfig) -> String {
    let model = match config.backend {
        ModelBackend::Local => config.local.model.trim(),
        ModelBackend::Api => config.api.model.trim(),
    };
    let model = if model.is_empty() { "未配置" } else { model };
    let style = match config.mode {
        TranslationMode::Conversational => "口语",
        TranslationMode::Academic => "学术",
    };
    format!("Translay\n模型：{model}\n风格：{style}")
}

fn next_translation_mode(mode: TranslationMode) -> TranslationMode {
    match mode {
        TranslationMode::Conversational => TranslationMode::Academic,
        TranslationMode::Academic => TranslationMode::Conversational,
    }
}

fn translation_mode_feedback(mode: TranslationMode) -> &'static str {
    match mode {
        TranslationMode::Conversational => "已切换至口语",
        TranslationMode::Academic => "已切换至学术",
    }
}

fn toggle_translation_mode(app: &tauri::AppHandle) -> Result<TranslationMode, String> {
    let config = app.state::<ModelConfigStore>();
    let credentials = app.state::<CredentialStore>();
    let mut next = config.get();
    next.mode = next_translation_mode(next.mode);
    let mode = next.mode;
    config.save(next)?;
    if let Err(error) = sync_model_backend_surfaces(app, config.inner(), credentials.inner()) {
        warn!(
            application = "Translay",
            capture_method = "tray-style-toggle",
            text_length = 0,
            elapsed_ms = 0,
            error_code = "TRAY_STYLE_SURFACE_SYNC_FAILED",
            %error,
            "translation style changed but dependent surfaces could not be refreshed"
        );
    }
    Ok(mode)
}

pub(super) fn setup_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let settings = MenuItem::with_id(app, "settings", "配置", true, None::<&str>)?;
    let (switch_label, switch_enabled) = {
        let config = app.state::<ModelConfigStore>().get();
        let credentials = app.state::<CredentialStore>();
        let target = alternate_model_backend(config.backend);
        (
            tray_model_switch_label(config.backend),
            model_backend_is_available(&config, credentials.inner(), target),
        )
    };
    let model_switch = MenuItem::with_id(
        app,
        "switch-model-backend",
        switch_label,
        switch_enabled,
        None::<&str>,
    )?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&settings, &model_switch, &separator, &quit])?;
    app.manage(TrayModelSwitch {
        item: model_switch.clone(),
    });
    let tooltip = tray_tooltip(&app.state::<ModelConfigStore>().get());
    let icon = tauri::image::Image::new(include_bytes!("../../icons/tray-32x32.rgba"), 32, 32);
    TrayIconBuilder::with_id(MAIN_TRAY_ID)
        .tooltip(tooltip)
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            let TrayIconEvent::Click {
                position,
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            else {
                return;
            };
            let app = tray.app_handle();
            let feedback = app.state::<TrayFeedbackManager>();
            if !feedback.accepts_toggle() {
                return;
            }
            match toggle_translation_mode(app) {
                Ok(mode) => {
                    if let Err(error) =
                        feedback.show(app, position, translation_mode_feedback(mode).to_owned())
                    {
                        warn!(
                            application = "Translay",
                            capture_method = "tray-style-feedback",
                            text_length = 0,
                            elapsed_ms = 0,
                            error_code = "TRAY_STYLE_FEEDBACK_FAILED",
                            %error,
                            "translation style changed but feedback could not be shown"
                        );
                    }
                }
                Err(error) => {
                    warn!(
                        application = "Translay",
                        capture_method = "tray-style-toggle",
                        text_length = 0,
                        elapsed_ms = 0,
                        error_code = "TRAY_STYLE_TOGGLE_FAILED",
                        %error,
                        "translation style could not be changed"
                    );
                }
            }
        })
        .on_menu_event(|app, event| match event.id.as_ref() {
            "settings" => {
                if show_settings_window(app).is_err() {
                    warn!(
                        application = "Translay",
                        capture_method = "settings-window",
                        text_length = 0,
                        elapsed_ms = 0,
                        error_code = "SETTINGS_WINDOW_SHOW_FAILED",
                        "model settings window could not be shown"
                    );
                }
            }
            "switch-model-backend" => {
                let config = app.state::<ModelConfigStore>();
                let credentials = app.state::<CredentialStore>();
                let target = alternate_model_backend(config.get().backend);
                match switch_model_backend(config.inner(), credentials.inner(), target).and_then(
                    |_| sync_model_backend_surfaces(app, config.inner(), credentials.inner()),
                ) {
                    Ok(_) => {}
                    Err(error) => {
                        warn!(
                            application = "Translay",
                            capture_method = "tray-model-switch",
                            text_length = 0,
                            elapsed_ms = 0,
                            error_code = "MODEL_BACKEND_SWITCH_FAILED",
                            %error,
                            "tray model switch was rejected"
                        );
                    }
                }
            }
            "quit" => {
                if let Some(shutdown) = app.try_state::<ShutdownCoordinator>() {
                    shutdown.request_exit(app);
                } else {
                    app.exit(0);
                }
            }
            _ => {}
        })
        .build(app)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{next_translation_mode, translation_mode_feedback, tray_tooltip};
    use crate::{model_config::ModelConfig, translation::TranslationMode};

    #[test]
    fn tooltip_describes_the_active_model_and_translation_style() {
        let mut config = ModelConfig::default();
        config.local.model = "deepseek-v4-flash".to_owned();
        assert_eq!(
            tray_tooltip(&config),
            "Translay\n模型：deepseek-v4-flash\n风格：口语"
        );

        config.backend = crate::model_config::ModelBackend::Api;
        config.api.model = "claude-sonnet".to_owned();
        config.mode = TranslationMode::Academic;
        assert_eq!(
            tray_tooltip(&config),
            "Translay\n模型：claude-sonnet\n风格：学术"
        );
    }

    #[test]
    fn tooltip_has_a_clear_fallback_when_the_active_model_is_empty() {
        assert_eq!(
            tray_tooltip(&ModelConfig::default()),
            "Translay\n模型：未配置\n风格：口语"
        );
    }

    #[test]
    fn tray_click_alternates_translation_styles_and_feedback() {
        assert_eq!(
            next_translation_mode(TranslationMode::Conversational),
            TranslationMode::Academic
        );
        assert_eq!(
            translation_mode_feedback(TranslationMode::Academic),
            "已切换至学术"
        );
        assert_eq!(
            next_translation_mode(TranslationMode::Academic),
            TranslationMode::Conversational
        );
        assert_eq!(
            translation_mode_feedback(TranslationMode::Conversational),
            "已切换至口语"
        );
    }
}
