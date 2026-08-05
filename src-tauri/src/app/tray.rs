use tauri::{
    Manager,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
};
use tracing::warn;

use crate::{
    credential_store::CredentialStore,
    model_config::{ModelConfig, ModelConfigStore},
};

use super::{
    model_backend::{
        alternate_model_backend, model_backend_is_available, switch_model_backend,
        sync_model_backend_surfaces, tray_model_switch_label,
    },
    shutdown::ShutdownCoordinator,
    windows::show_settings_window,
};

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
    tray_switch.refresh(config, credentials)
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
    let icon = tauri::image::Image::new(include_bytes!("../../icons/tray-32x32.rgba"), 32, 32);
    TrayIconBuilder::new()
        .tooltip("Translay")
        .icon(icon)
        .menu(&menu)
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
