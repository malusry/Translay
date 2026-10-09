use serde::Serialize;
use std::sync::Mutex;
use tauri::{Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder};

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum Phase {
    Loading,
    Ready,
    Setup,
    Already,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Snapshot {
    pub generation: u64,
    pub phase: Phase,
    pub visible: bool,
}

pub(super) struct Startup(Mutex<Snapshot>);
impl Startup {
    pub fn new(visible: bool) -> Self {
        Self(Mutex::new(Snapshot {
            generation: 1,
            phase: Phase::Loading,
            visible,
        }))
    }
    pub fn settle(&self, configured: bool) {
        self.0.lock().unwrap().phase = if configured {
            Phase::Ready
        } else {
            Phase::Setup
        };
    }
    pub fn snapshot(&self) -> Snapshot {
        self.0.lock().unwrap().clone()
    }
    pub fn repeat(&self) -> bool {
        let mut state = self.0.lock().unwrap();
        if state.visible || state.phase == Phase::Loading {
            return false;
        }
        state.generation += 1;
        state.phase = Phase::Already;
        state.visible = true;
        true
    }
    fn finish(&self, generation: u64) -> Option<bool> {
        let mut state = self.0.lock().unwrap();
        if state.generation != generation || !state.visible || state.phase == Phase::Loading {
            return None;
        }
        state.visible = false;
        Some(state.phase == Phase::Setup)
    }
}

pub(super) fn create(app: &tauri::AppHandle) -> Result<(), String> {
    let visible = app.state::<Startup>().snapshot().visible;
    let directory = super::windows::resolve_webview_data_directory(app, "startup")?;
    let window = WebviewWindowBuilder::new(
        app,
        "startup",
        WebviewUrl::App(
            if visible {
                "startup.html?present=1"
            } else {
                "startup.html"
            }
            .into(),
        ),
    )
    .title("Translay Startup")
    .inner_size(260.0, 120.0)
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
    .data_directory(directory)
    .build()
    .map_err(|e| e.to_string())?;
    window
        .set_ignore_cursor_events(true)
        .map_err(|e| e.to_string())?;
    // Show the transparent startup surface before constructing hidden utility WebViews.
    // Its small frontend paints after image decode; readiness still comes from settle().
    if visible {
        show(app)?;
    }
    Ok(())
}

pub(super) fn centered(position: (i32, i32), screen: (u32, u32), window: (u32, u32)) -> (i32, i32) {
    (
        position.0 + (screen.0 as i32 - window.0 as i32) / 2,
        position.1 + (screen.1 as i32 - window.1 as i32) / 2,
    )
}

fn show(app: &tauri::AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("startup")
        .ok_or("启动提示窗口不可用")?;
    // A frontend handshake can arrive while the early presentation is animating.
    // Do not resize/reposition the live WebView a second time during that animation.
    if window.is_visible().map_err(|e| e.to_string())? {
        return Ok(());
    }
    // Full primary-monitor bounds, not the cursor monitor or work area.
    let monitor = app
        .primary_monitor()
        .map_err(|e| e.to_string())?
        .ok_or("无法定位主显示器")?;
    let scale = monitor.scale_factor();
    let size = (
        (260.0 * scale).round() as u32,
        (120.0 * scale).round() as u32,
    );
    let target_size = PhysicalSize::new(size.0, size.1);
    if window.inner_size().map_err(|e| e.to_string())? != target_size {
        window.set_size(target_size).map_err(|e| e.to_string())?;
    }
    let origin = centered(
        (monitor.position().x, monitor.position().y),
        (monitor.size().width, monitor.size().height),
        size,
    );
    let target_position = PhysicalPosition::new(origin.0, origin.1);
    if window.outer_position().map_err(|e| e.to_string())? != target_position {
        window
            .set_position(target_position)
            .map_err(|e| e.to_string())?;
    }
    window.show().map_err(|e| e.to_string())
}

#[tauri::command]
pub(super) fn startup_status(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    reveal: bool,
) -> Result<Snapshot, String> {
    if window.label() != "startup" {
        return Err("Invalid startup caller".into());
    }
    let state = app.state::<Startup>().snapshot();
    if reveal && state.visible {
        show(&app)?;
    }
    Ok(state)
}

#[tauri::command]
pub(super) fn finish_startup(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    generation: u64,
) -> Result<(), String> {
    if window.label() != "startup" {
        return Err("Invalid startup caller".into());
    }
    finish(&app, generation)
}

fn finish(app: &tauri::AppHandle, generation: u64) -> Result<(), String> {
    if let Some(setup) = app.state::<Startup>().finish(generation) {
        if let Some(window) = app.get_webview_window("startup") {
            window.hide().map_err(|e| e.to_string())?;
        }
        if setup {
            super::windows::request_settings_window(app);
        }
    }
    Ok(())
}

// Presentation must never leave a stuck topmost surface if the WebView fails.
pub(super) fn schedule_safety_finish(app: &tauri::AppHandle) {
    let generation = app.state::<Startup>().snapshot().generation;
    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(15));
        let app = handle.clone();
        let _ = handle.run_on_main_thread(move || {
            let _ = finish(&app, generation);
        });
    });
}

pub(super) fn repeat(app: &tauri::AppHandle) {
    if app.state::<Startup>().repeat() {
        use tauri::Emitter;
        if let Some(window) = app.get_webview_window("startup") {
            let _ = window.emit("startup-repeat", ());
            let _ = show(app);
            schedule_safety_finish(app);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn primary_center_respects_origin_and_scaled_size() {
        assert_eq!(centered((0, 0), (1920, 1080), (260, 120)), (830, 480));
        assert_eq!(centered((-2560, 0), (2560, 1440), (390, 180)), (-1475, 630));
    }
    #[test]
    fn readiness_and_stale_finishes_are_guarded() {
        let s = Startup::new(true);
        assert_eq!(s.finish(1), None);
        assert!(!s.repeat());
        s.settle(false);
        assert_eq!(s.finish(1), Some(true));
        assert!(s.repeat());
        assert_eq!(s.snapshot().phase, Phase::Already);
        assert_eq!(s.finish(1), None);
        assert_eq!(s.finish(2), Some(false));
        assert_eq!(s.finish(2), None);
    }
    #[test]
    fn background_start_stays_silent_until_explicit_repeat() {
        let s = Startup::new(false);
        s.settle(true);
        assert!(!s.snapshot().visible);
        assert!(s.repeat());
        assert_eq!(s.snapshot().phase, Phase::Already);
    }
}
