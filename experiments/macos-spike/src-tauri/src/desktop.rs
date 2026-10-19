use std::sync::Mutex;

use tauri::{
    AppHandle, Emitter, Manager,
    image::Image,
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

use crate::{
    platform::{self, AccessibilityStatus},
    state::{Action, Snapshot, TranslationMode, toggles_selection},
};

const TRAY_ID: &str = "macos-spike";
struct Controls {
    selection: CheckMenuItem<tauri::Wry>,
    conversational: CheckMenuItem<tauri::Wry>,
    academic: CheckMenuItem<tauri::Wry>,
}

#[tauri::command]
fn get_spike_state(state: tauri::State<'_, Mutex<Snapshot>>) -> Snapshot {
    *state.lock().unwrap_or_else(|error| error.into_inner())
}

#[tauri::command]
fn get_accessibility_status() -> AccessibilityStatus {
    platform::accessibility_status()
}

#[tauri::command]
fn request_accessibility() -> Result<AccessibilityStatus, String> {
    platform::request_accessibility()
}

#[tauri::command]
fn open_system_settings() -> Result<(), String> {
    platform::open_system_settings()
}

fn snapshot(app: &AppHandle) -> Snapshot {
    *app.state::<Mutex<Snapshot>>()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

fn tray_image(enabled: bool) -> Image<'static> {
    let bytes = if enabled {
        include_bytes!("../../../../src-tauri/icons/tray-dark-32x32.rgba").as_slice()
    } else {
        include_bytes!("../../../../src-tauri/icons/tray-collapsed-dark-32x32.rgba").as_slice()
    };
    Image::new_owned(bytes.to_vec(), 32, 32)
}

fn publish(app: &AppHandle) -> tauri::Result<()> {
    let state = snapshot(app);
    let controls = app.state::<Controls>();
    controls
        .selection
        .set_checked(state.selection_icon_enabled)?;
    controls
        .conversational
        .set_checked(state.mode == TranslationMode::Conversational)?;
    controls
        .academic
        .set_checked(state.mode == TranslationMode::Academic)?;
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_icon(Some(tray_image(state.selection_icon_enabled)))?;
        tray.set_tooltip(Some(if state.selection_icon_enabled {
            "Translay · 划词图标开启"
        } else {
            "Translay · 划词图标关闭"
        }))?;
    }
    app.emit("spike-state", state)
}

fn apply(app: &AppHandle, action: Action) -> tauri::Result<()> {
    if snapshot(app).exiting {
        return Ok(());
    }
    app.state::<Mutex<Snapshot>>()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .apply(action);
    // Native check items toggle themselves before their callback. Re-selecting
    // the current mode must restore its check mark even when state is unchanged.
    publish(app)?;
    Ok(())
}

fn report(app: &AppHandle, result: tauri::Result<()>) {
    if let Err(error) = result {
        let _ = app.emit("spike-error", error.to_string());
        eprintln!("Mac prototype window/menu operation failed: {error}");
    }
}

fn show_settings(app: &AppHandle) -> tauri::Result<()> {
    if snapshot(app).exiting {
        return Ok(());
    }
    if let Some(window) = app.get_webview_window("settings") {
        window.unminimize()?;
        window.show()?;
        window.set_focus()?;
    }
    Ok(())
}

fn handle_menu(app: &AppHandle, event: tauri::menu::MenuEvent) {
    if snapshot(app).exiting {
        return;
    }
    let result = match event.id().as_ref() {
        "selection-icon" => apply(app, Action::ToggleSelectionIcon),
        "mode-conversational" => apply(app, Action::SelectMode(TranslationMode::Conversational)),
        "mode-academic" => apply(app, Action::SelectMode(TranslationMode::Academic)),
        "settings" => show_settings(app),
        "quit" => {
            report(app, apply(app, Action::Exit));
            app.exit(0);
            return;
        }
        _ => Ok(()),
    };
    report(app, result);
}

fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let selection =
        CheckMenuItem::with_id(app, "selection-icon", "划词图标", true, true, None::<&str>)?;
    let conversational = CheckMenuItem::with_id(
        app,
        "mode-conversational",
        "日常模式",
        true,
        true,
        None::<&str>,
    )?;
    let academic =
        CheckMenuItem::with_id(app, "mode-academic", "学习模式", true, false, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "配置…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出 Translay Mac Preview", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &selection,
            &PredefinedMenuItem::separator(app)?,
            &conversational,
            &academic,
            &PredefinedMenuItem::separator(app)?,
            &settings,
            &quit,
        ],
    )?;
    app.manage(Controls {
        selection,
        conversational,
        academic,
    });
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(tray_image(true))
        .icon_as_template(true)
        .tooltip("Translay · 划词图标开启")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button,
                button_state,
                ..
            } = event
            {
                if toggles_selection(
                    button == MouseButton::Left,
                    button_state == MouseButtonState::Up,
                ) {
                    report(
                        tray.app_handle(),
                        apply(tray.app_handle(), Action::ToggleSelectionIcon),
                    );
                }
            }
        })
        .build(app)?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn setup_app_menu(app: &AppHandle) -> tauri::Result<()> {
    use tauri::menu::Submenu;
    let application = Submenu::with_items(
        app,
        "Translay Mac Preview",
        true,
        &[
            &MenuItem::with_id(app, "settings", "配置…", true, Some("CmdOrCtrl+,"))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, Some("隐藏 Translay Mac Preview"))?,
            &PredefinedMenuItem::hide_others(app, Some("隐藏其他"))?,
            &PredefinedMenuItem::show_all(app, Some("全部显示"))?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(
                app,
                "quit",
                "退出 Translay Mac Preview",
                true,
                Some("CmdOrCtrl+Q"),
            )?,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        "编辑",
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?;
    let window = Submenu::with_items(
        app,
        "窗口",
        true,
        &[
            &PredefinedMenuItem::minimize(app, Some("最小化"))?,
            &PredefinedMenuItem::close_window(app, Some("关闭窗口"))?,
        ],
    )?;
    app.set_menu(Menu::with_items(app, &[&application, &edit, &window])?)?;
    Ok(())
}

pub fn run() {
    if !cfg!(target_os = "macos") {
        eprintln!("Mac 原型的原生窗口需要 macOS；浏览器预览请使用 npm run macos:preview。");
        std::process::exit(1);
    }
    let app = tauri::Builder::default()
        .manage(Mutex::new(Snapshot::default()))
        .invoke_handler(tauri::generate_handler![
            get_spike_state,
            get_accessibility_status,
            request_accessibility,
            open_system_settings
        ])
        .on_menu_event(handle_menu)
        .on_window_event(|window, event| {
            if window.label() == "settings" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    if !snapshot(window.app_handle()).exiting {
                        api.prevent_close();
                        report(window.app_handle(), window.hide());
                    }
                }
            }
        })
        .setup(|app| {
            #[cfg(target_os = "macos")]
            {
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
                setup_app_menu(app.handle())?;
            }
            setup_tray(app.handle())?;
            show_settings(app.handle())?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("无法启动 Translay Mac 原型");
    app.run(|_app, event| match event {
        tauri::RunEvent::ExitRequested {
            code: None, api, ..
        } => api.prevent_exit(),
        #[cfg(target_os = "macos")]
        tauri::RunEvent::Reopen { .. } => report(_app, show_settings(_app)),
        _ => {}
    });
}
