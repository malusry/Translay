use tauri::{
    Manager, PhysicalPosition, WebviewUrl, WebviewWindow, WebviewWindowBuilder, image::Image,
};

const TRAY_FEEDBACK_WIDTH: f64 = 240.0;
const TRAY_FEEDBACK_HEIGHT: f64 = 48.0;
const TRAY_FEEDBACK_EDGE_GAP: i32 = 8;

const SETTINGS_ICON_32: &[u8] =
    include_bytes!("../../../assets/icon-concepts/translay-desktop-v3/png/32x32.png");
const SETTINGS_ICON_40: &[u8] =
    include_bytes!("../../../assets/icon-concepts/translay-desktop-v3/png/40x40.png");
const SETTINGS_ICON_48: &[u8] =
    include_bytes!("../../../assets/icon-concepts/translay-desktop-v3/png/48x48.png");
const SETTINGS_ICON_64: &[u8] =
    include_bytes!("../../../assets/icon-concepts/translay-desktop-v3/png/64x64.png");

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

pub(super) fn create_tray_feedback_window(app: &tauri::AppHandle) -> Result<(), String> {
    if app.get_webview_window("tray-feedback").is_some() {
        return Ok(());
    }

    let data_directory = resolve_webview_data_directory(app, "tray-feedback")?;
    let window = WebviewWindowBuilder::new(
        app,
        "tray-feedback",
        WebviewUrl::App("index.html?view=tray-feedback".into()),
    )
    .title("Translay")
    .inner_size(TRAY_FEEDBACK_WIDTH, TRAY_FEEDBACK_HEIGHT)
    .visible(false)
    .focused(false)
    .focusable(false)
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
    .map_err(|error| format!("初始化托盘风格切换提示失败：{error}"))?;
    window
        .set_ignore_cursor_events(true)
        .map_err(|error| format!("设置托盘风格切换提示穿透失败：{error}"))?;
    Ok(())
}

pub(super) fn show_tray_feedback_window(
    app: &tauri::AppHandle,
    anchor: PhysicalPosition<f64>,
) -> Result<(), String> {
    let window = app
        .get_webview_window("tray-feedback")
        .ok_or_else(|| "找不到托盘风格切换提示".to_owned())?;
    let size = window
        .outer_size()
        .map_err(|error| format!("读取托盘风格切换提示尺寸失败：{error}"))?;
    let monitor = app
        .monitor_from_point(anchor.x, anchor.y)
        .map_err(|error| format!("定位托盘所在显示器失败：{error}"))?;
    let (x, y) = if let Some(monitor) = monitor {
        tray_feedback_position(
            anchor,
            size.width as i32,
            size.height as i32,
            monitor.work_area().position.x,
            monitor.work_area().position.y,
            monitor.work_area().size.width as i32,
            monitor.work_area().size.height as i32,
        )
    } else {
        (
            (anchor.x - f64::from(size.width) / 2.0).round() as i32,
            (anchor.y - f64::from(size.height) - f64::from(TRAY_FEEDBACK_EDGE_GAP)).round() as i32,
        )
    };
    window
        .set_position(PhysicalPosition::new(x, y))
        .map_err(|error| format!("定位托盘风格切换提示失败：{error}"))?;
    window
        .show()
        .map_err(|error| format!("显示托盘风格切换提示失败：{error}"))
}

pub(super) fn hide_tray_feedback_window(app: &tauri::AppHandle) -> Result<(), String> {
    app.get_webview_window("tray-feedback")
        .ok_or_else(|| "找不到托盘风格切换提示".to_owned())?
        .hide()
        .map_err(|error| format!("隐藏托盘风格切换提示失败：{error}"))
}

fn tray_feedback_position(
    anchor: PhysicalPosition<f64>,
    width: i32,
    height: i32,
    work_x: i32,
    work_y: i32,
    work_width: i32,
    work_height: i32,
) -> (i32, i32) {
    let work_right = work_x + work_width;
    let work_bottom = work_y + work_height;
    let centered_x = (anchor.x - f64::from(width) / 2.0).round() as i32;
    let centered_y = (anchor.y - f64::from(height) / 2.0).round() as i32;
    let (x, y) = if anchor.y >= f64::from(work_bottom) {
        (centered_x, work_bottom - height - TRAY_FEEDBACK_EDGE_GAP)
    } else if anchor.y < f64::from(work_y) {
        (centered_x, work_y + TRAY_FEEDBACK_EDGE_GAP)
    } else if anchor.x < f64::from(work_x) {
        (work_x + TRAY_FEEDBACK_EDGE_GAP, centered_y)
    } else if anchor.x >= f64::from(work_right) {
        (work_right - width - TRAY_FEEDBACK_EDGE_GAP, centered_y)
    } else {
        (
            centered_x,
            (anchor.y - f64::from(height) - f64::from(TRAY_FEEDBACK_EDGE_GAP)).round() as i32,
        )
    };
    (
        x.clamp(work_x, work_right - width),
        y.clamp(work_y, work_bottom - height),
    )
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
    refresh_settings_window_icon(&window)?;
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

fn refresh_settings_window_icon(window: &WebviewWindow) -> Result<(), String> {
    let scale_factor = window
        .scale_factor()
        .map_err(|error| format!("读取配置窗口缩放比例失败：{error}"))?;
    let size = settings_icon_size_for_scale_factor(scale_factor);
    let bytes = match size {
        32 => SETTINGS_ICON_32,
        40 => SETTINGS_ICON_40,
        48 => SETTINGS_ICON_48,
        _ => SETTINGS_ICON_64,
    };
    let icon =
        Image::from_bytes(bytes).map_err(|error| format!("读取配置窗口图标失败：{error}"))?;
    window
        .set_icon(icon)
        .map_err(|error| format!("更新配置窗口图标失败：{error}"))
}

fn settings_icon_size_for_scale_factor(scale_factor: f64) -> u32 {
    if scale_factor <= 1.0 {
        32
    } else if scale_factor <= 1.25 {
        40
    } else if scale_factor <= 1.5 {
        48
    } else {
        64
    }
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

#[cfg(test)]
mod tests {
    use super::{
        SETTINGS_ICON_32, SETTINGS_ICON_40, SETTINGS_ICON_48, SETTINGS_ICON_64,
        settings_icon_size_for_scale_factor, tray_feedback_position,
    };
    use tauri::{PhysicalPosition, image::Image};

    #[test]
    fn positions_tray_feedback_inside_the_bottom_work_area() {
        assert_eq!(
            tray_feedback_position(
                PhysicalPosition::new(1740.0, 1060.0),
                240,
                48,
                0,
                0,
                1920,
                1040,
            ),
            (1620, 984),
        );
    }

    #[test]
    fn positions_tray_feedback_below_a_top_taskbar() {
        assert_eq!(
            tray_feedback_position(
                PhysicalPosition::new(30.0, 20.0),
                240,
                48,
                0,
                40,
                1920,
                1040,
            ),
            (0, 48),
        );
    }

    #[test]
    fn selects_an_optical_icon_for_each_common_windows_scale() {
        assert_eq!(settings_icon_size_for_scale_factor(1.0), 32);
        assert_eq!(settings_icon_size_for_scale_factor(1.25), 40);
        assert_eq!(settings_icon_size_for_scale_factor(1.5), 48);
        assert_eq!(settings_icon_size_for_scale_factor(2.0), 64);
    }

    #[test]
    fn bundled_taskbar_icons_keep_their_native_pixel_sizes() {
        for (expected, bytes) in [
            (32, SETTINGS_ICON_32),
            (40, SETTINGS_ICON_40),
            (48, SETTINGS_ICON_48),
            (64, SETTINGS_ICON_64),
        ] {
            let image = Image::from_bytes(bytes).unwrap();
            assert_eq!(image.width(), expected);
            assert_eq!(image.height(), expected);
        }
    }
}
