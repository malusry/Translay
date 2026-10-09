use super::model_backend::{
    alternate_model_backend, model_backend_is_available, tray_model_switch_label,
};
use crate::{
    credential_store::CredentialStore, model_config::ModelConfigStore, models::ScreenRect,
    overlay_manager::monitor_metrics, overlay_policy::scale_for_dpi,
};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::{
    Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

const WIDTH: i32 = 188;
const HEIGHT: i32 = 160;

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MenuSnapshot {
    generation: u64,
    open: bool,
    model_label: String,
    model_enabled: bool,
    selection_enabled: bool,
}

#[derive(Default)]
struct MenuState {
    snapshot: MenuSnapshot,
    action_in_flight: bool,
    stopping: bool,
}

#[derive(Clone, Default)]
pub(super) struct TrayMenu(Arc<Mutex<MenuState>>);

struct MenuAction(TrayMenu);

impl Drop for MenuAction {
    fn drop(&mut self) {
        self.0
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .action_in_flight = false;
    }
}

impl TrayMenu {
    fn snapshot(&self) -> MenuSnapshot {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .snapshot
            .clone()
    }

    fn accepts_open(&self) -> bool {
        let state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        !state.stopping && !state.action_in_flight
    }

    fn claim_action(&self, generation: u64) -> Option<MenuAction> {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if state.stopping
            || state.action_in_flight
            || !state.snapshot.open
            || state.snapshot.generation != generation
        {
            return None;
        }
        state.snapshot.open = false;
        state.action_in_flight = true;
        Some(MenuAction(self.clone()))
    }

    pub(super) fn stop(&self) {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        state.stopping = true;
        state.snapshot.open = false;
    }
}

pub(super) fn create(app: &tauri::AppHandle) -> Result<(), String> {
    if app.get_webview_window("tray-menu").is_some() {
        return Ok(());
    }
    let window = WebviewWindowBuilder::new(
        app,
        "tray-menu",
        WebviewUrl::App("index.html?view=tray-menu".into()),
    )
    .title("Translay")
    .inner_size(WIDTH as f64, HEIGHT as f64)
    .visible(false)
    .focused(false)
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(false)
    .maximizable(false)
    .minimizable(false)
    .closable(false)
    .data_directory(super::windows::resolve_webview_data_directory(
        app,
        "tray-menu",
    )?)
    .build()
    .map_err(|e| e.to_string())?;
    let handle = app.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let generation = handle.state::<TrayMenu>().snapshot().generation;
            if let Err(error) = close_on_window_thread(&handle, Some(generation)) {
                tracing::warn!(%error, "tray menu close failed");
            }
        }
        if matches!(event, tauri::WindowEvent::Focused(false)) {
            if let Some(window) = handle.get_webview_window("tray-menu") {
                if window.is_visible().unwrap_or(false) && !window.is_focused().unwrap_or(false) {
                    let generation = handle.state::<TrayMenu>().snapshot().generation;
                    if let Err(error) = close_on_window_thread(&handle, Some(generation)) {
                        tracing::warn!(%error, "tray menu focus dismissal failed");
                    }
                }
            }
        }
    });
    Ok(())
}

pub(super) async fn show(
    app: &tauri::AppHandle,
    point: PhysicalPosition<f64>,
    rect: tauri::Rect,
) -> Result<(), String> {
    let handle = app.clone();
    super::windows::on_window_thread(app, move || show_on_window_thread(&handle, point, rect)).await
}

pub(super) fn request_show(
    app: &tauri::AppHandle,
    point: PhysicalPosition<f64>,
    rect: tauri::Rect,
) {
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = show(&handle, point, rect).await {
            super::tray::tray_action_error(&error);
        }
    });
}

fn show_on_window_thread(
    app: &tauri::AppHandle,
    point: PhysicalPosition<f64>,
    rect: tauri::Rect,
) -> Result<(), String> {
    let state = app.state::<TrayMenu>();
    if !state.accepts_open() {
        return Ok(());
    }
    // Invalidate before hiding, so focus callbacks from the previous session
    // cannot consume an action or a painted acknowledgement for the next one.
    close_on_window_thread(app, None)?;
    let config = app.state::<ModelConfigStore>().get();
    let credentials = app.state::<CredentialStore>();
    let anchor = ScreenRect {
        left: point.x as i32,
        top: point.y as i32,
        right: point.x as i32 + 1,
        bottom: point.y as i32 + 1,
    };
    let (_, dpi) = monitor_metrics(anchor, ::windows::Win32::Foundation::HWND::default())?;
    let bounds = monitor_bounds(anchor)?;
    let origin = rect.position.to_physical::<i32>(dpi as f64 / 96.0);
    let size = rect.size.to_physical::<i32>(dpi as f64 / 96.0);
    let anchor = if size.width > 0 && size.height > 0 {
        ScreenRect {
            left: origin.x,
            top: origin.y,
            right: origin.x + size.width,
            bottom: origin.y + size.height,
        }
    } else {
        anchor
    };
    let width = scale_for_dpi(WIDTH, dpi);
    let height = scale_for_dpi(HEIGHT, dpi);
    let position = placement(anchor, bounds, width, height, 0);
    let window = app
        .get_webview_window("tray-menu")
        .ok_or("菜单窗口未创建")?;
    window.hide().map_err(|e| e.to_string())?;
    window
        .set_position(PhysicalPosition::new(position.0, position.1))
        .map_err(|e| e.to_string())?;
    window
        .set_size(PhysicalSize::new(width as u32, height as u32))
        .map_err(|e| e.to_string())?;
    let model_enabled = model_backend_is_available(
        &config,
        credentials.inner(),
        alternate_model_backend(config.backend),
    );
    let snapshot = {
        let mut current = state.0.lock().unwrap_or_else(|e| e.into_inner());
        if current.stopping {
            return Ok(());
        }
        current.snapshot = MenuSnapshot {
            generation: current.snapshot.generation + 1,
            open: true,
            model_label: tray_model_switch_label(config.backend).into(),
            model_enabled,
            selection_enabled: config.selection_icon_enabled,
        };
        current.snapshot.clone()
    };
    if let Err(error) = window.emit("tray-menu-open", snapshot.clone()) {
        close_on_window_thread(app, Some(snapshot.generation))?;
        return Err(error.to_string());
    }
    Ok(())
}

// Context menus may overlap the taskbar. rcWork would push an icon-anchored
// menu upward again after placement; only the physical monitor edge is a limit.
fn monitor_bounds(anchor: ScreenRect) -> Result<ScreenRect, String> {
    use ::windows::Win32::{
        Foundation::RECT,
        Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromRect},
    };
    let rect = RECT {
        left: anchor.left,
        top: anchor.top,
        right: anchor.right,
        bottom: anchor.bottom,
    };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        let monitor = MonitorFromRect(&rect, MONITOR_DEFAULTTONEAREST);
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return Err("读取托盘菜单屏幕边界失败".into());
        }
    }
    Ok(ScreenRect {
        left: info.rcMonitor.left,
        top: info.rcMonitor.top,
        right: info.rcMonitor.right,
        bottom: info.rcMonitor.bottom,
    })
}

fn placement(
    anchor: ScreenRect,
    work: ScreenRect,
    width: i32,
    height: i32,
    gap: i32,
) -> (i32, i32) {
    // The visible bottom-left corner starts at the icon center, like a context menu.
    // Exclude transparent shadow padding when anchoring either side.
    let inset = height * 12 / HEIGHT;
    let center_x = anchor.left + (anchor.right - anchor.left) / 2;
    let center_y = anchor.top + (anchor.bottom - anchor.top) / 2;
    let rightward = center_x - inset;
    let x = if rightward + width <= work.right {
        rightward
    } else {
        center_x - width + inset
    }
    .clamp(work.left, (work.right - width).max(work.left));
    let above = center_y - height + inset - gap;
    let y = if above >= work.top {
        above
    } else {
        center_y + gap - inset
    };
    (x, y.clamp(work.top, (work.bottom - height).max(work.top)))
}

pub(super) async fn close(app: &tauri::AppHandle, generation: Option<u64>) -> Result<bool, String> {
    let handle = app.clone();
    super::windows::on_window_thread(app, move || close_on_window_thread(&handle, generation)).await
}

// Refresh is also called from synchronous configuration IPC. Capture its current
// generation before dispatch so a late refresh cannot close a newly opened menu.
pub(super) fn request_close(app: &tauri::AppHandle) {
    let generation = app.state::<TrayMenu>().snapshot().generation;
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = close(&handle, Some(generation)).await {
            tracing::warn!(%error, "tray menu refresh dismissal failed");
        }
    });
}

fn close_on_window_thread(app: &tauri::AppHandle, generation: Option<u64>) -> Result<bool, String> {
    {
        let state = app.state::<TrayMenu>();
        let mut current = state.0.lock().unwrap_or_else(|e| e.into_inner());
        if !current.snapshot.open || generation.is_some_and(|g| g != current.snapshot.generation) {
            return Ok(false);
        }
        current.snapshot.open = false;
    }
    if let Some(window) = app.get_webview_window("tray-menu") {
        window.hide().map_err(|e| e.to_string())?;
    }
    #[cfg(debug_assertions)]
    native::record_manual_event(app, "menu-dismissed");
    Ok(true)
}

fn authorize(window: &WebviewWindow) -> Result<(), String> {
    if window.label() == "tray-menu" {
        Ok(())
    } else {
        Err("菜单命令来源无效".into())
    }
}

#[tauri::command]
pub(super) fn get_tray_menu(
    window: WebviewWindow,
    state: tauri::State<'_, TrayMenu>,
) -> Result<MenuSnapshot, String> {
    authorize(&window)?;
    Ok(state.snapshot())
}

#[tauri::command]
pub(super) async fn tray_menu_painted(
    app: tauri::AppHandle,
    window: WebviewWindow,
    generation: u64,
) -> Result<(), String> {
    authorize(&window)?;
    let handle = app.clone();
    super::windows::on_window_thread(&app, move || {
        let current = handle.state::<TrayMenu>().snapshot();
        if current.open && current.generation == generation {
            let result = window.show().and_then(|_| window.set_focus());
            if let Err(error) = result {
                close_on_window_thread(&handle, Some(generation))?;
                return Err(error.to_string());
            }
        }
        Ok(())
    })
    .await
}

#[tauri::command]
pub(super) async fn dismiss_tray_menu(
    app: tauri::AppHandle,
    window: WebviewWindow,
    generation: u64,
) -> Result<(), String> {
    authorize(&window)?;
    close(&app, Some(generation)).await.map(|_| ())
}

#[tauri::command]
pub(super) async fn tray_menu_action(
    app: tauri::AppHandle,
    window: WebviewWindow,
    generation: u64,
    action: String,
) -> Result<(), String> {
    authorize(&window)?;
    if !matches!(
        action.as_str(),
        "settings" | "switch-model-backend" | "selection-icon" | "quit"
    ) {
        return Err("未知菜单操作".into());
    }
    let handle = app.clone();
    let claim = super::windows::on_window_thread(&app, move || {
        let Some(claim) = handle.state::<TrayMenu>().claim_action(generation) else {
            return Ok(None);
        };
        // Hide before starting settings or any operation that can change focus.
        window.hide().map_err(|e| e.to_string())?;
        Ok(Some(claim))
    })
    .await?;
    let Some(_claim) = claim else {
        return Ok(());
    };
    #[cfg(debug_assertions)]
    native::record_manual_event(&app, &format!("action:{action}"));
    // The claim lives through the complete worker action, including refresh and
    // settings focus. Opening is accepted again only once the claim is released.
    let result = super::tray::perform_action(&app, &action).await;
    if let Err(error) = &result {
        super::tray::tray_action_error(error);
    }
    result
}

#[cfg(debug_assertions)]
#[path = "tray_menu_native.rs"]
mod native;
#[cfg(debug_assertions)]
pub(super) use native::run_native_smoke;
#[cfg(debug_assertions)]
pub(super) use native::{
    MANUAL_TEST_LABEL, NativeAcceptanceData, before_tray_frame, cleanup_native_credential,
    is_manual_acceptance, record_manual_event, record_tray_visuals,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn open_state(generation: u64) -> TrayMenu {
        let menu = TrayMenu::default();
        menu.0.lock().unwrap().snapshot = MenuSnapshot {
            generation,
            open: true,
            ..Default::default()
        };
        menu
    }

    #[test]
    fn action_consumes_session_once_and_blocks_open_until_worker_finishes() {
        let menu = open_state(7);
        let claim = menu.claim_action(7).unwrap();
        assert!(!menu.snapshot().open);
        assert!(!menu.accepts_open());
        assert!(menu.claim_action(7).is_none());
        drop(claim);
        assert!(menu.accepts_open());
        assert!(menu.claim_action(7).is_none());
    }

    #[test]
    fn stale_action_and_worker_failure_leave_next_session_usable() {
        let menu = open_state(8);
        assert!(menu.claim_action(7).is_none());
        assert!(menu.snapshot().open);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _claim = menu.claim_action(8).unwrap();
            panic!("synthetic action worker failure");
        }));
        assert!(result.is_err());
        assert!(menu.accepts_open());
        menu.0.lock().unwrap().snapshot = MenuSnapshot {
            generation: 9,
            open: true,
            ..Default::default()
        };
        assert!(menu.claim_action(8).is_none());
        assert!(menu.claim_action(9).is_some());
    }

    #[test]
    fn shutdown_invalidates_menu_and_late_actions_even_after_claim_drops() {
        let menu = open_state(3);
        let claim = menu.claim_action(3).unwrap();
        menu.stop();
        drop(claim);
        assert!(!menu.snapshot().open);
        assert!(!menu.accepts_open());
        assert!(menu.claim_action(3).is_none());
    }
    #[test]
    fn taskbar_icon_is_not_pushed_above_the_work_area() {
        for dpi in [96, 120, 144, 192] {
            let s = |v| scale_for_dpi(v, dpi);
            let screen = ScreenRect {
                left: 0,
                top: 0,
                right: s(1920),
                bottom: s(1080),
            };
            let work = ScreenRect {
                bottom: s(1032),
                ..screen
            };
            let icon = ScreenRect {
                left: s(1600),
                top: s(1044),
                right: s(1624),
                bottom: s(1068),
            };
            let (_, y) = placement(icon, screen, s(WIDTH), s(HEIGHT), 0);
            assert_eq!(y + s(HEIGHT) - s(12), s(1056));
            let (_, old_y) = placement(icon, work, s(WIDTH), s(HEIGHT), 0);
            assert!(
                old_y < y,
                "work-area clamping reproduces the unwanted upward shift"
            );
        }
    }
    #[test]
    fn menu_visible_corner_starts_at_the_icon_center() {
        let anchor = ScreenRect {
            left: 800,
            top: 800,
            right: 824,
            bottom: 824,
        };
        let work = ScreenRect {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        let (x, y) = placement(anchor, work, WIDTH, HEIGHT, 0);
        assert_eq!(x + 12, anchor.left + 12);
        assert_eq!(anchor.top + 12, y + HEIGHT - 12);
        let right_work = ScreenRect { right: 900, ..work };
        let (x, y) = placement(anchor, right_work, WIDTH, HEIGHT, 0);
        assert_eq!(x + WIDTH - 12, anchor.left + 12);
        assert_eq!(y + HEIGHT - 12, anchor.top + 12);
    }
    #[test]
    fn placement_stays_inside_scaled_negative_work_areas() {
        for dpi in [96, 120, 144, 192] {
            let s = |v| scale_for_dpi(v, dpi);
            let work = ScreenRect {
                left: -s(1920),
                top: -s(1080),
                right: 0,
                bottom: 0,
            };
            for (x, y) in [
                (work.left, work.top),
                (work.right - 1, work.top),
                (work.left, work.bottom - 1),
                (work.right - 1, work.bottom - 1),
            ] {
                let (px, py) = placement(
                    ScreenRect {
                        left: x,
                        top: y,
                        right: x + 1,
                        bottom: y + 1,
                    },
                    work,
                    s(WIDTH),
                    s(HEIGHT),
                    s(6),
                );
                assert!(
                    px >= work.left
                        && py >= work.top
                        && px + s(WIDTH) <= work.right
                        && py + s(HEIGHT) <= work.bottom
                );
            }
        }
    }
}
