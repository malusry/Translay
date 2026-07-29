mod commands;
mod model_backend;
mod shutdown;
mod tray;
mod windows;

use std::{io, sync::Arc};

use ::windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use tauri::Manager;
use tauri_plugin_global_shortcut::ShortcutState;
use tracing::{error, info, warn};

use crate::{
    capture_coordinator::CaptureCoordinator, credential_store::CredentialStore,
    hotkey_service::HotkeyService, latest_capture_store::LatestCaptureStore,
    model_config::ModelConfigStore, overlay_manager::OverlayManager,
    single_instance::SingleInstanceGuard, translation_service::TranslationService,
};

use shutdown::ShutdownCoordinator;
use tray::setup_tray;
use windows::{create_overlay_window, show_settings_window};

pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_target(false)
        .without_time()
        .try_init();

    let _instance_guard = match SingleInstanceGuard::acquire() {
        Ok(Some(guard)) => guard,
        Ok(None) => {
            info!(
                application = "Translay",
                capture_method = "startup",
                text_length = 0,
                elapsed_ms = 0,
                error_code = "INSTANCE_ALREADY_RUNNING",
                "a Translay instance is already running"
            );
            return;
        }
        Err(error) => {
            eprintln!("Translay 无法建立单实例保护：{error}");
            return;
        }
    };

    // Tauri also declares DPI awareness in its Windows manifest. Calling this
    // before window creation makes the coordinate contract explicit;
    // ERROR_ACCESS_DENIED simply means the manifest already set it.
    if let Err(error) =
        unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) }
    {
        warn!(
            application = "Translay",
            capture_method = "startup",
            text_length = 0,
            elapsed_ms = 0,
            error_code = %format!("0x{:08X}", error.code().0 as u32),
            "DPI awareness was already set or unavailable"
        );
    }

    let latest_capture = LatestCaptureStore::default();
    let overlay = OverlayManager::new(latest_capture.clone());
    let coordinator = CaptureCoordinator::new(overlay.clone());
    let setup_coordinator = coordinator.clone();
    let handler_coordinator = coordinator.clone();
    let hotkeys = Arc::new(HotkeyService::default());
    let setup_hotkeys = hotkeys.clone();
    let shutdown = ShutdownCoordinator::new(hotkeys.clone(), coordinator.clone());

    let builder = tauri::Builder::default()
        .manage(latest_capture)
        .manage(overlay)
        .manage(coordinator)
        .manage(shutdown)
        .invoke_handler(tauri::generate_handler![
            commands::overlay_frontend_ready,
            commands::get_latest_capture,
            commands::ack_capture,
            commands::dismiss_overlay,
            commands::set_overlay_hovered,
            commands::fit_overlay_height,
            commands::retry_capture,
            commands::copy_translation,
            commands::get_model_config,
            commands::get_model_api_key_status,
            commands::save_model_provider_config,
            commands::activate_model_backend,
            commands::save_translation_preferences,
            commands::clear_model_api_key,
            commands::test_model_connection,
            commands::start_settings_dragging,
            commands::hide_settings_window
        ])
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        handler_coordinator.trigger(app.clone());
                    }
                })
                .build(),
        );

    let app = builder
        .setup(move |app| {
            create_overlay_window(app.handle())
                .map_err(|error| io::Error::other(format!("创建翻译浮层失败：{error}")))?;
            let config_store = ModelConfigStore::load(app.handle())
                .map_err(|error| io::Error::other(format!("加载模型配置失败：{error}")))?;
            let credential_store = CredentialStore;
            if let Err(error) =
                credential_store.migrate_legacy_api_key(&config_store.get().api.base_url)
            {
                warn!(
                    application = "Translay",
                    capture_method = "credential-migration",
                    text_length = 0,
                    elapsed_ms = 0,
                    error_code = "API_KEY_MIGRATION_FAILED",
                    %error,
                    "legacy API key could not be migrated to its provider scope"
                );
            }
            let translation_service =
                TranslationService::new(config_store.clone(), credential_store.clone());
            app.manage(config_store);
            app.manage(credential_store);
            app.manage(translation_service);

            setup_coordinator
                .overlay()
                .configure_native_style(app.handle())
                .map_err(|error| io::Error::other(format!("配置浮层 Win32 样式失败：{error}")))?;

            setup_tray(app)
                .map_err(|error| io::Error::other(format!("创建系统托盘失败：{error}")))?;
            setup_hotkeys
                .register(app.handle())
                .map_err(|error| io::Error::other(format!("注册全局快捷键失败：{error}")))?;
            if std::env::args_os().any(|argument| argument == "--settings") {
                show_settings_window(app.handle())
                    .map_err(|error| io::Error::other(format!("显示配置失败：{error}")))?;
            }
            info!(
                application = "Translay",
                capture_method = "startup",
                text_length = 0,
                elapsed_ms = 0,
                error_code = "",
                "Translay is running in the system tray"
            );
            Ok(())
        })
        .build(tauri::generate_context!())
        .unwrap_or_else(|error| {
            eprintln!("Translay 启动失败：{error}");
            std::process::exit(1);
        });

    app.run(move |handle, event| match event {
        tauri::RunEvent::WindowEvent {
            label,
            event: tauri::WindowEvent::CloseRequested { api, .. },
            ..
        } if label == "settings" => {
            api.prevent_close();
            if let Some(window) = handle.get_webview_window("settings") {
                let _ = window.hide();
            }
        }
        tauri::RunEvent::Exit => {
            if hotkeys.unregister(handle).is_err() {
                error!(
                    application = "Translay",
                    capture_method = "shutdown",
                    text_length = 0,
                    elapsed_ms = 0,
                    error_code = "HOTKEY_UNREGISTER_FAILED",
                    "global shortcut unregister failed"
                );
            }
            // Exercise the idempotent path during every clean shutdown.
            let _ = hotkeys.unregister(handle);
        }
        _ => {}
    });
}
