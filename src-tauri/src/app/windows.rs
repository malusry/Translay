use std::sync::{Arc, Mutex};
use tauri::{
    Manager, PhysicalPosition, WebviewUrl, WebviewWindow, WebviewWindowBuilder, image::Image,
};

// Only settings needs lazy WebView creation. Reserve an open without holding a
// state lock across native calls; hiding or shutdown invalidates its presentation.
#[derive(Clone, Default)]
pub(super) struct SettingsWindow(Arc<Mutex<SettingsLifecycle>>);

#[derive(Default)]
struct SettingsLifecycle {
    opening: Option<SettingsCompletion>,
    opening_generation: u64,
    generation: u64,
    stopping: bool,
    #[cfg(debug_assertions)]
    before_create: Option<SettingsHook>,
    #[cfg(debug_assertions)]
    before_present: Option<SettingsHook>,
}

#[cfg(debug_assertions)]
type SettingsHook = Box<dyn FnOnce() -> Result<(), String> + Send>;

struct SettingsOpen {
    state: SettingsWindow,
    generation: u64,
    completion: tokio::sync::watch::Sender<Option<Result<(), String>>>,
    result: Option<Result<(), String>>,
}

type SettingsCompletion = tokio::sync::watch::Receiver<Option<Result<(), String>>>;

enum SettingsRequest {
    Start(SettingsOpen),
    Join(SettingsCompletion, Option<u64>),
    Stopped,
}

impl SettingsWindow {
    fn begin_open(&self, expected_generation: Option<u64>) -> SettingsRequest {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if state.stopping || expected_generation.is_some_and(|g| g != state.generation) {
            return SettingsRequest::Stopped;
        }
        if let Some(completion) = &state.opening {
            // A new user open after hiding must wait for the cancelled worker,
            // then start a fresh presentation. Earlier joined requests stay cancelled.
            let reopen = (state.opening_generation != state.generation).then_some(state.generation);
            return SettingsRequest::Join(completion.clone(), reopen);
        }
        let (send, receive) = tokio::sync::watch::channel(None);
        state.opening = Some(receive);
        state.opening_generation = state.generation;
        SettingsRequest::Start(SettingsOpen {
            state: self.clone(),
            generation: state.generation,
            completion: send,
            result: None,
        })
    }

    fn invalidate(&self) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).generation += 1;
    }

    fn is_current(&self, generation: u64) -> bool {
        let state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        !state.stopping && state.generation == generation
    }

    pub(super) fn stop(&self) {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        state.stopping = true;
        state.generation += 1;
    }

    // Native acceptance injects failures/barriers here, without touching disk,
    // credentials or native APIs. There is no IPC for these debug-only hooks.
    #[cfg(debug_assertions)]
    pub(super) fn before_create(&self, hook: SettingsHook) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .before_create = Some(hook);
    }

    #[cfg(debug_assertions)]
    pub(super) fn before_present(&self, hook: SettingsHook) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .before_present = Some(hook);
    }

    #[cfg(debug_assertions)]
    pub(super) fn is_opening(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .opening
            .is_some()
    }
}

impl SettingsOpen {
    fn is_current(&self) -> bool {
        self.state.is_current(self.generation)
    }

    #[cfg(debug_assertions)]
    fn run_create_hook(&self) -> Result<(), String> {
        let hook = {
            let mut state = self.state.0.lock().unwrap_or_else(|e| e.into_inner());
            state.before_create.take()
        };
        if let Some(hook) = hook {
            hook()?;
        }
        Ok(())
    }
}

impl Drop for SettingsOpen {
    fn drop(&mut self) {
        self.state
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .opening = None;
        // Also wake coalesced callers if a worker unwinds or dispatch is dropped.
        self.completion.send_replace(Some(
            self.result
                .take()
                .unwrap_or_else(|| Err("配置窗口操作提前结束".into())),
        ));
    }
}

// Serialize presentation/close with native focus events on the window thread.
// This helper must never contain WebviewWindowBuilder::build().
pub(super) async fn on_window_thread<T: Send + 'static>(
    app: &tauri::AppHandle,
    operation: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (send, receive) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let _ = send.send(operation());
    })
    .map_err(|e| format!("投递窗口操作失败：{e}"))?;
    receive.await.map_err(|e| format!("窗口操作未完成：{e}"))?
}

const TRAY_FEEDBACK_WIDTH: f64 = 64.0;
const TRAY_FEEDBACK_HEIGHT: f64 = 48.0;
const TRAY_FEEDBACK_VISIBLE_GAP: f64 = 2.0;
// The 12px vertical shadow margin may overlap the taskbar by 10px,
// retaining a 2px gap below the visible label at each monitor scale.
const TRAY_FEEDBACK_EDGE_GAP: f64 = TRAY_FEEDBACK_VISIBLE_GAP - 12.0;

const SETTINGS_ICON_32: &[u8] = include_bytes!("../../icons/32x32.png");
const SETTINGS_ICON_40: &[u8] = include_bytes!("../../icons/40x40.png");
const SETTINGS_ICON_48: &[u8] = include_bytes!("../../icons/48x48.png");
const SETTINGS_ICON_64: &[u8] = include_bytes!("../../icons/64x64.png");

pub(super) fn create_overlay_window(app: &tauri::AppHandle) -> Result<(), String> {
    let directory = resolve_webview_data_directory(app, "overlay")?;
    create_overlay_window_with_data(app, directory)
}

pub(super) fn create_overlay_window_with_data(
    app: &tauri::AppHandle,
    data_directory: std::path::PathBuf,
) -> Result<(), String> {
    if app.get_webview_window("overlay").is_some() {
        return Ok(());
    }

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
    icon_rect: Option<tauri::Rect>,
) -> Result<(), String> {
    let window = app
        .get_webview_window("tray-feedback")
        .ok_or_else(|| "找不到托盘风格切换提示".to_owned())?;
    // Resolve the clicked native surface before showing our own window.
    let overflow = tray_overflow_rect(anchor);
    let monitor = app
        .monitor_from_point(anchor.x, anchor.y)
        .map_err(|error| format!("定位托盘所在显示器失败：{error}"))?;
    let scale = monitor.as_ref().map(|m| m.scale_factor()).unwrap_or(1.0);
    let edge_gap = (TRAY_FEEDBACK_EDGE_GAP * scale).round() as i32;
    let size = tauri::PhysicalSize::new(
        (TRAY_FEEDBACK_WIDTH * scale).round() as u32,
        (TRAY_FEEDBACK_HEIGHT * scale).round() as u32,
    );
    window
        .set_size(size)
        .map_err(|error| format!("调整托盘提示尺寸失败：{error}"))?;
    let icon = icon_rect.and_then(|rect| feedback_icon_rect(rect, anchor, scale));
    let (x, y) = if let (Some(monitor), Some(icon)) = (monitor.as_ref(), icon) {
        icon_feedback_position(
            icon,
            crate::models::ScreenRect {
                left: monitor.position().x,
                top: monitor.position().y,
                right: monitor.position().x + monitor.size().width as i32,
                bottom: monitor.position().y + monitor.size().height as i32,
            },
            crate::models::ScreenRect {
                left: monitor.work_area().position.x,
                top: monitor.work_area().position.y,
                right: monitor.work_area().position.x + monitor.work_area().size.width as i32,
                bottom: monitor.work_area().position.y + monitor.work_area().size.height as i32,
            },
            overflow.is_some(),
            scale,
        )
    } else if let Some(monitor) = monitor {
        let fallback = tray_feedback_position(
            anchor,
            size.width as i32,
            size.height as i32,
            monitor.work_area().position.x,
            monitor.work_area().position.y,
            monitor.work_area().size.width as i32,
            monitor.work_area().size.height as i32,
            edge_gap,
        );
        overflow
            .map(|panel| {
                overflow_feedback_position(
                    anchor,
                    size.width as i32,
                    size.height as i32,
                    panel,
                    crate::models::ScreenRect {
                        left: monitor.work_area().position.x,
                        top: monitor.work_area().position.y,
                        right: monitor.work_area().position.x
                            + monitor.work_area().size.width as i32,
                        bottom: monitor.work_area().position.y
                            + monitor.work_area().size.height as i32,
                    },
                    // Bring the visible label 3 logical pixels into the panel
                    // edge above/below; the feedback window stays mouse-transparent.
                    (-15.0 * scale).round() as i32,
                )
            })
            .unwrap_or(fallback)
    } else {
        (
            (anchor.x - f64::from(size.width) / 2.0).round() as i32,
            (anchor.y - f64::from(size.height) - f64::from(edge_gap)).round() as i32,
        )
    };
    window
        .set_position(PhysicalPosition::new(x, y))
        .map_err(|error| format!("定位托盘风格切换提示失败：{error}"))?;
    // Moving across monitors can trigger a native DPI resize; reapply the
    // destination monitor's physical size before the window becomes visible.
    window
        .set_size(size)
        .map_err(|error| format!("调整托盘提示尺寸失败：{error}"))?;
    window
        .show()
        .map_err(|error| format!("显示托盘风格切换提示失败：{error}"))
}

// Windows tray-icon obtains this rectangle through Shell_NotifyIconGetRect.
// Reject missing/stale geometry rather than anchoring to a different surface.
fn feedback_icon_rect(
    rect: tauri::Rect,
    anchor: PhysicalPosition<f64>,
    scale: f64,
) -> Option<crate::models::ScreenRect> {
    let origin = rect.position.to_physical::<i32>(scale);
    let size = rect.size.to_physical::<i32>(scale);
    if size.width <= 0 || size.height <= 0 || !anchor.x.is_finite() || !anchor.y.is_finite() {
        return None;
    }
    let icon = crate::models::ScreenRect {
        left: origin.x,
        top: origin.y,
        right: origin.x.checked_add(size.width)?,
        bottom: origin.y.checked_add(size.height)?,
    };
    (anchor.x >= f64::from(icon.left)
        && anchor.x < f64::from(icon.right)
        && anchor.y >= f64::from(icon.top)
        && anchor.y < f64::from(icon.bottom))
    .then_some(icon)
}

// Both tray surfaces measure the same visible gap from the clicked icon. Only
// screen edges constrain the pill; transparent shadow space may cross them.
fn icon_feedback_position(
    icon: crate::models::ScreenRect,
    screen: crate::models::ScreenRect,
    work: crate::models::ScreenRect,
    overflow: bool,
    scale: f64,
) -> (i32, i32) {
    let px = |logical: f64| (logical * scale).round() as i32;
    let width = px(TRAY_FEEDBACK_WIDTH);
    let height = px(TRAY_FEEDBACK_HEIGHT);
    let inset_x = px(14.);
    let inset_y = px(12.);
    let gap = px(TRAY_FEEDBACK_VISIBLE_GAP);
    let center_x = icon.left + icon.width() / 2;
    let center_y = icon.top + icon.height() / 2;
    let min_x = screen.left + gap - inset_x;
    let min_y = screen.top + gap - inset_y;
    let max_x = (screen.right - gap - width + inset_x).max(min_x);
    let max_y = (screen.bottom - gap - height + inset_y).max(min_y);
    let centered_x = (center_x - width / 2).clamp(min_x, max_x);
    let centered_y = (center_y - height / 2).clamp(min_y, max_y);
    let above = (centered_x, icon.top - gap - height + inset_y);
    let below = (centered_x, icon.bottom + gap - inset_y);
    let left = (icon.left - gap - width + inset_x, centered_y);
    let right = (icon.right + gap - inset_x, centered_y);
    let candidates = if overflow || center_y >= work.bottom {
        [above, below, left, right]
    } else if center_y < work.top {
        [below, above, left, right]
    } else if center_x < work.left {
        [right, left, above, below]
    } else if center_x >= work.right {
        [left, right, above, below]
    } else {
        [above, below, left, right]
    };
    let (x, y) = candidates
        .into_iter()
        .find(|&(x, y)| {
            x + inset_x >= screen.left + gap
                && x + width - inset_x <= screen.right - gap
                && y + inset_y >= screen.top + gap
                && y + height - inset_y <= screen.bottom - gap
        })
        .unwrap_or(candidates[0]);
    (x.clamp(min_x, max_x), y.clamp(min_y, max_y))
}

// Windows versions expose different overflow hosts. If the clicked root isn't
// recognized, retain click-based placement rather than reusing stale geometry.
fn tray_overflow_rect(anchor: PhysicalPosition<f64>) -> Option<crate::models::ScreenRect> {
    use windows::Win32::{
        Foundation::{POINT, RECT},
        UI::WindowsAndMessaging::{
            GA_ROOT, GetAncestor, GetClassNameW, GetWindowRect, IsWindowVisible, WindowFromPoint,
        },
    };
    let point = POINT {
        x: anchor.x.round() as i32,
        y: anchor.y.round() as i32,
    };
    let root = unsafe { GetAncestor(WindowFromPoint(point), GA_ROOT) };
    if root.0.is_null() || !unsafe { IsWindowVisible(root) }.as_bool() {
        return None;
    }
    let mut class = [0u16; 128];
    let length = unsafe { GetClassNameW(root, &mut class) };
    let name = String::from_utf16_lossy(&class[..length.max(0) as usize]);
    if !matches!(
        name.as_str(),
        "NotifyIconOverflowWindow" | "TopLevelWindowForOverflowXamlIsland"
    ) {
        return None;
    }
    let mut rect = RECT::default();
    unsafe { GetWindowRect(root, &mut rect) }.ok()?;
    if rect.right <= rect.left
        || rect.bottom <= rect.top
        || point.x < rect.left
        || point.x >= rect.right
        || point.y < rect.top
        || point.y >= rect.bottom
    {
        return None;
    }
    Some(crate::models::ScreenRect {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
    })
}

fn overflow_feedback_position(
    anchor: PhysicalPosition<f64>,
    width: i32,
    height: i32,
    panel: crate::models::ScreenRect,
    work: crate::models::ScreenRect,
    gap: i32,
) -> (i32, i32) {
    let center_y = (anchor.y - f64::from(height) / 2.0).round() as i32;
    let center_x = (anchor.x - f64::from(width) / 2.0).round() as i32;
    let left = panel.left - gap - width;
    let right = panel.right + gap;
    let above = panel.top - gap - height;
    let below = panel.bottom + gap;
    let (x, y) = if above >= work.top {
        (center_x, above)
    } else if below + height <= work.bottom {
        (center_x, below)
    } else if left >= work.left {
        (left, center_y)
    } else {
        (right, center_y)
    };
    (
        x.clamp(work.left, (work.right - width).max(work.left)),
        y.clamp(work.top, (work.bottom - height).max(work.top)),
    )
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
    gap: i32,
) -> (i32, i32) {
    let work_right = work_x + work_width;
    let work_bottom = work_y + work_height;
    let centered_x = (anchor.x - f64::from(width) / 2.0).round() as i32;
    let centered_y = (anchor.y - f64::from(height) / 2.0).round() as i32;
    let (x, y) = if anchor.y >= f64::from(work_bottom) {
        (centered_x, work_bottom - height - gap)
    } else if anchor.y < f64::from(work_y) {
        (centered_x, work_y + gap)
    } else if anchor.x < f64::from(work_x) {
        (work_x + gap, centered_y)
    } else if anchor.x >= f64::from(work_right) {
        (work_right - width - gap, centered_y)
    } else {
        (
            centered_x,
            (anchor.y - f64::from(height) - f64::from(gap)).round() as i32,
        )
    };
    // Keep the visible label inside the work area while letting its transparent
    // shadow margin overlap the taskbar; clamping the full window pushes it away.
    let shadow_overlap = gap.min(0);
    let min_x = work_x + shadow_overlap;
    let min_y = work_y + shadow_overlap;
    (
        x.clamp(min_x, (work_right - width - shadow_overlap).max(min_x)),
        y.clamp(min_y, (work_bottom - height - shadow_overlap).max(min_y)),
    )
}

pub(super) fn resolve_webview_data_directory(
    app: &tauri::AppHandle,
    window_label: &str,
) -> Result<std::path::PathBuf, String> {
    #[cfg(debug_assertions)]
    if let Some(data) = app.try_state::<super::tray_menu::NativeAcceptanceData>() {
        let directory = data.0.join("webviews").join(window_label);
        std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        return Ok(directory);
    }
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

pub(super) async fn show_settings_window(app: &tauri::AppHandle) -> Result<(), String> {
    let mut expected_generation = None;
    loop {
        let open = match app
            .state::<SettingsWindow>()
            .begin_open(expected_generation)
        {
            SettingsRequest::Stopped => return Ok(()),
            SettingsRequest::Join(mut completion, reopen) => {
                let result = loop {
                    if let Some(result) = completion.borrow_and_update().clone() {
                        break result;
                    }
                    completion
                        .changed()
                        .await
                        .map_err(|e| format!("配置窗口操作未完成：{e}"))?;
                };
                if let Some(generation) = reopen {
                    expected_generation = Some(generation);
                    continue;
                }
                return result;
            }
            SettingsRequest::Start(open) => open,
        };
        let handle = app.clone();
        // All callers (including synchronous startup IPC/event callbacks) enter this
        // worker. Merely declaring the caller async would not protect Windows build().
        return tauri::async_runtime::spawn_blocking(move || {
            let mut open = open;
            let result = open_settings_on_worker(&handle, &open);
            // Release the reservation before waking joiners. New opens can reuse the
            // existing window; every joined menu action has awaited the same focus.
            open.result = Some(result.clone());
            drop(open);
            result
        })
        .await
        .map_err(|e| format!("配置窗口操作执行失败：{e}"))?;
    }
}

fn open_settings_on_worker(handle: &tauri::AppHandle, open: &SettingsOpen) -> Result<(), String> {
    if !open.is_current() {
        return Ok(());
    }
    #[cfg(debug_assertions)]
    open.run_create_hook()?;
    if !open.is_current() {
        return Ok(());
    }
    let window = ensure_settings_window(&handle)?;
    let presentation_app = handle.clone();
    let state = open.state.clone();
    let generation = open.generation;
    tauri::async_runtime::block_on(on_window_thread(&handle, move || {
        let current = || state.is_current(generation);
        if !current() {
            return Ok(());
        }
        #[cfg(debug_assertions)]
        {
            let hook = state
                .0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .before_present
                .take();
            if let Some(hook) = hook {
                hook()?;
            }
        }
        let result = present_settings_window(&window);
        // A worker may have requested exit while this callback was running.
        if !current() {
            hide_settings_window(&presentation_app)?;
        }
        result
    }))
}

// For setup and synchronous callbacks: dispatch and return, never wait for build.
pub(super) fn request_settings_window(app: &tauri::AppHandle) {
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = show_settings_window(&handle).await {
            super::tray::tray_action_error(&error);
        }
    });
}

fn ensure_settings_window(app: &tauri::AppHandle) -> Result<WebviewWindow, String> {
    let window = match app.get_webview_window("settings") {
        Some(window) => window,
        None => {
            let data_directory = resolve_webview_data_directory(app, "settings")?;
            let window = WebviewWindowBuilder::new(
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
            .map_err(|error| format!("创建配置窗口失败：{error}"))?;
            let handle = app.clone();
            window.on_window_event(move |event| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    if let Err(error) = hide_settings_window(&handle) {
                        tracing::warn!(%error, "settings close failed");
                    }
                }
            });
            window
        }
    };
    Ok(window)
}

fn present_settings_window(window: &WebviewWindow) -> Result<(), String> {
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
    app.state::<SettingsWindow>().invalidate();
    app.get_webview_window("settings")
        .ok_or_else(|| "找不到配置窗口".to_owned())?
        .hide()
        .map_err(|error| format!("隐藏配置窗口失败：{error}"))
}

pub(super) fn minimize_settings_window(app: &tauri::AppHandle) -> Result<(), String> {
    app.state::<SettingsWindow>().invalidate();
    app.get_webview_window("settings")
        .ok_or_else(|| "找不到配置窗口".to_owned())?
        .minimize()
        .map_err(|error| format!("最小化配置窗口失败：{error}"))
}

#[cfg(test)]
mod tests {
    use super::{SettingsOpen, SettingsRequest, SettingsWindow};

    fn start(state: &SettingsWindow) -> SettingsOpen {
        let SettingsRequest::Start(open) = state.begin_open(None) else {
            panic!("expected a fresh open");
        };
        open
    }

    #[test]
    fn settings_open_is_single_flight_and_released_after_error_or_unwind() {
        let state = SettingsWindow::default();
        let open = start(&state);
        let SettingsRequest::Join(completion, None) = state.begin_open(None) else {
            panic!("duplicate must join");
        };
        assert!(completion.borrow().is_none());
        assert!(open.is_current());
        drop(open);
        assert!(completion.borrow().as_ref().unwrap().is_err());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _open = start(&state);
            panic!("synthetic worker failure");
        }));
        assert!(result.is_err());
        assert!(start(&state).is_current());
    }

    #[test]
    fn hiding_or_minimizing_invalidates_late_presentation_without_blocking_retry() {
        let state = SettingsWindow::default();
        let open = start(&state);
        state.invalidate();
        assert!(!open.is_current());
        let SettingsRequest::Join(_, Some(generation)) = state.begin_open(None) else {
            panic!("open after hide must wait to present anew");
        };
        drop(open);
        state.invalidate();
        assert!(matches!(
            state.begin_open(Some(generation)),
            SettingsRequest::Stopped
        ));
        assert!(start(&state).is_current());
    }

    #[test]
    fn shutdown_rejects_current_and_all_later_settings_opens() {
        let state = SettingsWindow::default();
        let open = start(&state);
        state.stop();
        assert!(!open.is_current());
        drop(open);
        assert!(matches!(state.begin_open(None), SettingsRequest::Stopped));
        state.stop();
        assert!(matches!(state.begin_open(None), SettingsRequest::Stopped));
    }

    #[test]
    fn joined_settings_callers_receive_the_same_failure_and_can_retry() {
        let state = SettingsWindow::default();
        let mut open = start(&state);
        let SettingsRequest::Join(completion, None) = state.begin_open(None) else {
            panic!("duplicate must join");
        };
        open.result = Some(Err("synthetic display failure".into()));
        drop(open);
        assert_eq!(
            *completion.borrow(),
            Some(Err("synthetic display failure".into()))
        );
        assert!(start(&state).is_current());
    }

    use super::{
        SETTINGS_ICON_32, SETTINGS_ICON_40, SETTINGS_ICON_48, SETTINGS_ICON_64,
        settings_icon_size_for_scale_factor, tray_feedback_position,
    };
    use tauri::{PhysicalPosition, image::Image};

    #[test]
    fn taskbar_and_each_overflow_row_keep_the_same_visible_icon_gap() {
        use crate::models::ScreenRect;
        for scale in [1., 1.25, 1.5, 2., 3.] {
            let px = |logical: f64| (logical * scale).round() as i32;
            let screen = ScreenRect {
                left: px(-1200.),
                top: px(-900.),
                right: 0,
                bottom: 0,
            };
            let work = ScreenRect {
                bottom: px(-48.),
                ..screen
            };
            // First the direct taskbar, then three rows in a hidden panel.
            for (top, overflow) in [(-36., false), (-250., true), (-210., true), (-170., true)] {
                let icon = ScreenRect {
                    left: px(-200.),
                    top: px(top),
                    right: px(-176.),
                    bottom: px(top + 24.),
                };
                let (x, y) = super::icon_feedback_position(icon, screen, work, overflow, scale);
                let visible_bottom = y + px(super::TRAY_FEEDBACK_HEIGHT) - px(12.);
                assert_eq!(icon.top - visible_bottom, px(2.));
                assert_eq!(
                    x + px(super::TRAY_FEEDBACK_WIDTH) / 2,
                    icon.left + icon.width() / 2
                );
                assert!(y + px(12.) >= screen.top);
            }
        }
    }

    #[test]
    fn icon_feedback_keeps_taskbar_directions_and_overflow_edge_fallbacks() {
        use crate::models::ScreenRect;
        for scale in [1., 1.25, 1.5, 2., 3.] {
            let px = |logical: f64| (logical * scale).round() as i32;
            let screen = ScreenRect {
                left: px(-1000.),
                top: px(-800.),
                right: 0,
                bottom: 0,
            };
            let work = ScreenRect {
                left: px(-952.),
                top: px(-752.),
                right: px(-48.),
                bottom: px(-48.),
            };
            // Direct bottom/top/left/right; hidden panel top/left/right corners.
            for (left, top, overflow, side) in [
                (-100., -36., false, 0),
                (-100., -788., false, 1),
                (-988., -100., false, 2),
                (-36., -100., false, 3),
                (-100., -798., true, 1),
                (-1000., -300., true, 0),
                (-24., -300., true, 0),
            ] {
                let icon = ScreenRect {
                    left: px(left),
                    top: px(top),
                    right: px(left + 24.),
                    bottom: px(top + 24.),
                };
                let (x, y) = super::icon_feedback_position(icon, screen, work, overflow, scale);
                let visible = ScreenRect {
                    left: x + px(14.),
                    top: y + px(12.),
                    right: x + px(super::TRAY_FEEDBACK_WIDTH) - px(14.),
                    bottom: y + px(super::TRAY_FEEDBACK_HEIGHT) - px(12.),
                };
                assert!(visible.left >= screen.left && visible.right <= screen.right);
                assert!(visible.top >= screen.top && visible.bottom <= screen.bottom);
                let gaps = [
                    icon.top - visible.bottom,
                    visible.top - icon.bottom,
                    visible.left - icon.right,
                    icon.left - visible.right,
                ];
                assert_eq!(gaps[side], px(2.));
            }
        }
    }

    #[test]
    fn feedback_icon_rect_rejects_empty_or_stale_rectangles_and_scales_logical_ones() {
        let rect = tauri::Rect {
            position: tauri::LogicalPosition::new(-100., -200.).into(),
            size: tauri::LogicalSize::new(24., 24.).into(),
        };
        let icon =
            super::feedback_icon_rect(rect, PhysicalPosition::new(-140., -290.), 1.5).unwrap();
        assert_eq!(
            (icon.left, icon.top, icon.right, icon.bottom),
            (-150, -300, -114, -264)
        );
        assert!(super::feedback_icon_rect(rect, PhysicalPosition::new(100., 100.), 1.5).is_none());
        assert!(
            super::feedback_icon_rect(rect, PhysicalPosition::new(f64::NAN, -290.), 1.5).is_none()
        );
        assert!(
            super::feedback_icon_rect(
                tauri::Rect {
                    size: tauri::PhysicalSize::new(0, 0).into(),
                    ..rect
                },
                PhysicalPosition::new(-140., -290.),
                1.5
            )
            .is_none()
        );
    }

    #[test]
    fn compact_feedback_stays_close_to_panel_at_common_scales() {
        use crate::models::ScreenRect;
        for scale in [1., 1.25, 1.5, 2., 3.] {
            let width = (super::TRAY_FEEDBACK_WIDTH * scale).round() as i32;
            let height = (super::TRAY_FEEDBACK_HEIGHT * scale).round() as i32;
            let panel = ScreenRect {
                left: (500. * scale) as i32,
                top: (600. * scale) as i32,
                right: (740. * scale) as i32,
                bottom: (790. * scale) as i32,
            };
            let (x, y) = super::overflow_feedback_position(
                PhysicalPosition::new(620. * scale, 720. * scale),
                width,
                height,
                panel,
                ScreenRect {
                    left: 0,
                    top: 0,
                    right: (1200. * scale) as i32,
                    bottom: (900. * scale) as i32,
                },
                (-15. * scale).round() as i32,
            );
            assert!((x + width / 2 - (620. * scale) as i32).abs() <= 1);
            let visible_bottom = y + height - (12. * scale).round() as i32;
            assert!((visible_bottom - panel.top - (3. * scale).round() as i32).abs() <= 1);
        }
    }

    #[test]
    fn overflow_feedback_avoids_panel_at_each_available_side() {
        use crate::models::ScreenRect;
        let work = ScreenRect {
            left: 0,
            top: 0,
            right: 1000,
            bottom: 800,
        };
        for (panel, expected) in [
            (
                ScreenRect {
                    left: 800,
                    top: 500,
                    right: 990,
                    bottom: 700,
                },
                (450, 438),
            ),
            (
                ScreenRect {
                    left: 5,
                    top: 500,
                    right: 195,
                    bottom: 700,
                },
                (450, 438),
            ),
            (
                ScreenRect {
                    left: 5,
                    top: 500,
                    right: 995,
                    bottom: 700,
                },
                (450, 438),
            ),
            (
                ScreenRect {
                    left: 5,
                    top: 5,
                    right: 995,
                    bottom: 200,
                },
                (450, 210),
            ),
            (
                ScreenRect {
                    left: 800,
                    top: 5,
                    right: 990,
                    bottom: 795,
                },
                (690, 574),
            ),
            (
                ScreenRect {
                    left: 5,
                    top: 5,
                    right: 195,
                    bottom: 795,
                },
                (205, 574),
            ),
        ] {
            assert_eq!(
                super::overflow_feedback_position(
                    PhysicalPosition::new(500., 600.),
                    100,
                    52,
                    panel,
                    work,
                    10
                ),
                expected
            );
        }
    }

    #[test]
    fn overflow_feedback_clamps_on_negative_monitor_and_scaled_edges() {
        use crate::models::ScreenRect;
        for scale in [1, 2, 3] {
            let work = ScreenRect {
                left: -1000 * scale,
                top: -800 * scale,
                right: 0,
                bottom: 0,
            };
            let panel = ScreenRect {
                left: -200 * scale,
                top: -150 * scale,
                right: -10 * scale,
                bottom: -5 * scale,
            };
            let width = 100 * scale;
            let height = 52 * scale;
            let (x, y) = super::overflow_feedback_position(
                PhysicalPosition::new(-50. * scale as f64, -6. * scale as f64),
                width,
                height,
                panel,
                work,
                10 * scale,
            );
            assert!(
                x >= work.left
                    && y >= work.top
                    && x + width <= work.right
                    && y + height <= work.bottom
            );
            assert!(y + height <= panel.top);
        }
    }

    #[test]
    fn compact_feedback_keeps_visible_clearance_at_all_taskbar_edges_and_scales() {
        for scale in [1., 1.25, 1.5, 2., 3.] {
            let px = |logical: f64| (logical * scale).round() as i32;
            let work_x = px(-1000.);
            let work_y = px(-800.);
            let width = px(super::TRAY_FEEDBACK_WIDTH);
            let height = px(super::TRAY_FEEDBACK_HEIGHT);
            // Click near a corner on each taskbar side, exercising both the
            // placement direction and clamping of the visible pill, not its shadow.
            for (anchor, side, expected_gap) in [
                (PhysicalPosition::new(-1., f64::from(px(10.))), 0, px(2.)),
                (
                    PhysicalPosition::new(f64::from(work_x + 1), f64::from(work_y - px(10.))),
                    1,
                    px(2.),
                ),
                (
                    PhysicalPosition::new(f64::from(work_x - px(10.)), f64::from(work_y + 1)),
                    2,
                    px(4.),
                ),
                (PhysicalPosition::new(f64::from(px(10.)), -1.), 3, px(4.)),
            ] {
                let (x, y) = tray_feedback_position(
                    anchor,
                    width,
                    height,
                    work_x,
                    work_y,
                    -work_x,
                    -work_y,
                    px(super::TRAY_FEEDBACK_EDGE_GAP),
                );
                let visible_left = x + px(14.);
                let visible_right = x + width - px(14.);
                let visible_top = y + px(12.);
                let visible_bottom = y + height - px(12.);
                assert!(visible_left > work_x && visible_right < 0);
                assert!(visible_top > work_y && visible_bottom < 0);
                let clearances = [
                    -visible_bottom,
                    visible_top - work_y,
                    visible_left - work_x,
                    -visible_right,
                ];
                assert!((clearances[side] - expected_gap).abs() <= 1);
            }
        }
    }

    #[test]
    fn positions_tray_feedback_close_to_bottom_taskbar() {
        assert_eq!(
            tray_feedback_position(
                PhysicalPosition::new(1740.0, 1060.0),
                64,
                48,
                0,
                0,
                1920,
                1040,
                -10,
            ),
            (1708, 1002),
        );
    }

    #[test]
    fn positions_tray_feedback_below_a_top_taskbar() {
        assert_eq!(
            tray_feedback_position(
                PhysicalPosition::new(30.0, 20.0),
                64,
                48,
                0,
                40,
                1920,
                1040,
                -10,
            ),
            (-2, 30),
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
