use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

pub(super) fn create_overlay_window(app: &tauri::AppHandle) -> Result<(), String> {
    if app.get_webview_window("overlay").is_some() {
        return Ok(());
    }

    let data_directory = resolve_webview_data_directory(app, "overlay")?;

    WebviewWindowBuilder::new(app, "overlay", WebviewUrl::App("index.html".into()))
        .title("Translay")
        .inner_size(148.0, 58.0)
        .visible(false)
        .focused(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .closable(false)
        .transparent(true)
        .shadow(false)
        .data_directory(data_directory)
        .build()
        .map(|_| ())
        .map_err(|error| format!("初始化浮层 WebView 失败：{error}"))
}

pub(super) fn create_selection_button_window(app: &tauri::AppHandle) -> Result<(), String> {
    if app.get_webview_window("selection-button").is_some() {
        return Ok(());
    }

    let data_directory = resolve_webview_data_directory(app, "selection-button")?;

    WebviewWindowBuilder::new(
        app,
        "selection-button",
        WebviewUrl::App("index.html?view=selection-button".into()),
    )
    .title("Translay")
    .inner_size(40.0, 40.0)
    .visible(false)
    .focused(false)
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(false)
    .maximizable(false)
    .minimizable(false)
    .closable(false)
    .transparent(true)
    .shadow(false)
    .data_directory(data_directory)
    .build()
    .map(|_| ())
    .map_err(|error| format!("初始化划词翻译按钮失败：{error}"))
}

fn resolve_webview_data_directory(
    app: &tauri::AppHandle,
    window_label: &str,
) -> Result<std::path::PathBuf, String> {
    let primary_error = match app.path().app_local_data_dir() {
        Ok(app_data_dir) => {
            let primary = app_data_dir.join("webviews").join(window_label).join("v3");
            match std::fs::create_dir_all(&primary) {
                Ok(()) => return Ok(primary),
                Err(error) => error.to_string(),
            }
        }
        Err(error) => error.to_string(),
    };

    let executable = std::env::current_exe()
        .map_err(|error| format!("定位 Translay 可执行文件失败：{error}"))?;
    let executable_dir = executable
        .parent()
        .ok_or_else(|| "Translay 可执行文件没有父目录".to_owned())?;
    let fallback = executable_dir
        .join(".translay-webviews")
        .join(window_label)
        .join("v3");
    std::fs::create_dir_all(&fallback).map_err(|fallback_error| {
        format!("创建 WebView 数据目录失败；标准目录：{primary_error}；备用目录：{fallback_error}")
    })?;
    Ok(fallback)
}

pub(super) fn show_settings_window(app: &tauri::AppHandle) -> Result<(), String> {
    let window = match app.get_webview_window("settings") {
        Some(window) => window,
        None => {
            let data_directory = resolve_webview_data_directory(app, "settings")?;
            WebviewWindowBuilder::new(
                app,
                "settings",
                WebviewUrl::App("index.html?view=settings".into()),
            )
            .title("Translay 设置")
            .inner_size(680.0, 570.0)
            .visible(false)
            .focused(true)
            .center()
            .decorations(false)
            .always_on_top(false)
            .skip_taskbar(false)
            .resizable(false)
            .maximizable(false)
            .minimizable(true)
            .closable(false)
            .shadow(true)
            .data_directory(data_directory)
            .build()
            .map_err(|error| format!("创建配置窗口失败：{error}"))?
        }
    };
    window
        .unminimize()
        .map_err(|error| format!("恢复配置窗口失败：{error}"))?;
    window
        .show()
        .map_err(|error| format!("显示配置窗口失败：{error}"))?;
    window
        .set_focus()
        .map_err(|error| format!("聚焦配置窗口失败：{error}"))
}

pub(super) fn start_settings_dragging(app: &tauri::AppHandle) -> Result<(), String> {
    app.get_webview_window("settings")
        .ok_or_else(|| "找不到配置窗口".to_owned())?
        .start_dragging()
        .map_err(|error| format!("拖动配置窗口失败：{error}"))
}

pub(super) fn hide_settings_window(app: &tauri::AppHandle) -> Result<(), String> {
    app.get_webview_window("settings")
        .ok_or_else(|| "找不到配置窗口".to_owned())?
        .hide()
        .map_err(|error| format!("隐藏配置窗口失败：{error}"))
}

pub(super) fn minimize_settings_window(app: &tauri::AppHandle) -> Result<(), String> {
    app.get_webview_window("settings")
        .ok_or_else(|| "找不到配置窗口".to_owned())?
        .minimize()
        .map_err(|error| format!("最小化配置窗口失败：{error}"))
}
