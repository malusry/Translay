use std::{
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use tauri::{AppHandle, Emitter, Manager};
use tracing::{info, warn};
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM},
    Graphics::Gdi::{CombineRgn, CreateRectRgn, DeleteObject, RGN_DIFF, RGN_ERROR, SetWindowRgn},
    UI::{
        Input::KeyboardAndMouse::{
            GetAsyncKeyState, GetDoubleClickTime, VK_CONTROL, VK_DOWN, VK_END, VK_ESCAPE, VK_HOME,
            VK_LBUTTON, VK_LEFT, VK_NEXT, VK_PRIOR, VK_RIGHT, VK_SHIFT, VK_UP,
        },
        WindowsAndMessaging::{
            CallNextHookEx, DispatchMessageW, GA_ROOT, GWL_EXSTYLE, GetAncestor, GetCursorPos,
            GetForegroundWindow, GetWindowLongPtrW, GetWindowThreadProcessId, HWND_TOPMOST, MSG,
            MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx, PM_REMOVE, PeekMessageW, QS_ALLINPUT,
            SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_SHOWWINDOW, SetWindowLongPtrW,
            SetWindowPos, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, WH_MOUSE_LL,
            WM_MOUSEHWHEEL, WM_MOUSEWHEEL, WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
            WindowFromPoint,
        },
    },
};

use crate::{
    capture_session::{CaptureRequest, CaptureSession},
    config::{
        SELECTION_BUTTON_CAPTURE_TIMEOUT, SELECTION_BUTTON_GAP, SELECTION_BUTTON_GLYPH_SIZE,
        SELECTION_BUTTON_SETTLE_DELAY, SELECTION_BUTTON_SIZE, SELECTION_BUTTON_VISIBLE_DURATION,
    },
    foreground_context::ForegroundContext,
    model_config::ModelConfigStore,
    models::{CapturedSelection, ScreenRect},
    overlay_manager::{cursor_anchor, monitor_metrics},
    overlay_policy::scale_for_dpi,
    selection_service::{MouseSelectionHint, SelectionService},
    translation::TranslationMode,
};

const INPUT_POLL_INTERVAL: Duration = Duration::from_millis(10);
const DRAG_THRESHOLD: i32 = 4;
const KEY_A: i32 = 0x41;
static SCROLL_EPOCH: AtomicU64 = AtomicU64::new(0);

#[derive(Clone)]
pub struct SelectionButtonManager {
    inner: Arc<SelectionButtonState>,
}

struct SelectionButtonState {
    candidate: Mutex<Option<CapturedSelection>>,
    bounds: Mutex<Option<ScreenRect>>,
    excluded: Mutex<Vec<ScreenRect>>,
    lifecycle: Mutex<()>,
    probe_sessions: CaptureSession,
    generation: AtomicU64,
    visibility_revision: AtomicU64,
    running: AtomicBool,
    enabled: AtomicBool,
    monitor_epoch: AtomicU64,
    wake: Condvar,
    wake_lock: Mutex<()>,
}

#[derive(Clone, Copy)]
struct MouseRelease {
    point: POINT,
    at: Instant,
}

impl Default for SelectionButtonManager {
    fn default() -> Self {
        Self {
            inner: Arc::new(SelectionButtonState {
                candidate: Mutex::new(None),
                bounds: Mutex::new(None),
                excluded: Mutex::new(Vec::new()),
                lifecycle: Mutex::new(()),
                probe_sessions: CaptureSession::default(),
                generation: AtomicU64::new(0),
                visibility_revision: AtomicU64::new(0),
                running: AtomicBool::new(false),
                enabled: AtomicBool::new(true),
                monitor_epoch: AtomicU64::new(0),
                wake: Condvar::new(),
                wake_lock: Mutex::new(()),
            }),
        }
    }
}

impl SelectionButtonManager {
    pub fn initialize_enabled(&self, enabled: bool) {
        self.inner.enabled.store(enabled, Ordering::Release);
    }

    pub fn is_enabled(&self) -> bool {
        self.inner.enabled.load(Ordering::Acquire)
    }

    fn is_enabled_for(&self, epoch: u64) -> bool {
        self.is_enabled() && self.inner.monitor_epoch.load(Ordering::Acquire) == epoch
    }

    pub fn set_enabled(&self, app: &AppHandle, enabled: bool) -> Result<(), String> {
        if !self.transition_enabled(enabled) {
            return Ok(());
        }
        if !enabled {
            self.emit_visibility(app, false);
            hide_native_button(app)?;
        }
        Ok(())
    }

    fn transition_enabled(&self, enabled: bool) -> bool {
        let _lifecycle = self
            .inner
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let _wake = self
            .inner
            .wake_lock
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if self.is_enabled() == enabled {
            return false;
        }
        self.inner.enabled.store(enabled, Ordering::Release);
        self.inner.monitor_epoch.fetch_add(1, Ordering::AcqRel);
        self.inner.probe_sessions.cancel_current();
        self.inner.generation.fetch_add(1, Ordering::AcqRel);
        self.inner
            .candidate
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        self.inner
            .bounds
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        self.inner
            .excluded
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clear();
        self.inner.wake.notify_all();
        true
    }

    pub fn configure_native_style(&self, app: &AppHandle) -> Result<(), String> {
        let hwnd = selection_button_hwnd(app)?;
        // SAFETY: hwnd belongs to the pre-created Tauri tool window.
        unsafe {
            let current = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            let updated = (current | WS_EX_NOACTIVATE.0 as isize | WS_EX_TOOLWINDOW.0 as isize)
                & !(WS_EX_APPWINDOW.0 as isize);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, updated);
            let applied = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            if applied & WS_EX_NOACTIVATE.0 as isize == 0
                || applied & WS_EX_TOOLWINDOW.0 as isize == 0
                || applied & WS_EX_APPWINDOW.0 as isize != 0
            {
                return Err("划词按钮的非激活窗口样式配置失败".to_owned());
            }
        }
        Ok(())
    }

    pub fn start(&self, app: AppHandle) -> Result<(), String> {
        if self.inner.running.swap(true, Ordering::AcqRel) {
            return Ok(());
        }

        let manager = self.clone();
        thread::Builder::new()
            .name("translay-selection-monitor".into())
            .spawn(move || run_input_monitor(app, manager.clone()))
            .map(|_| ())
            .map_err(|error| {
                self.inner.running.store(false, Ordering::Release);
                format!("无法启动划词检测：{error}")
            })
    }

    pub fn stop(&self, app: &AppHandle) {
        let _wake = self
            .inner
            .wake_lock
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        self.inner.running.store(false, Ordering::Release);
        self.inner.enabled.store(false, Ordering::Release);
        self.inner.monitor_epoch.fetch_add(1, Ordering::AcqRel);
        self.inner.wake.notify_all();
        drop(_wake);
        self.inner.probe_sessions.cancel_current();
        let _ = self.hide(app);
    }

    pub fn dismiss(&self, app: &AppHandle) {
        self.inner.probe_sessions.cancel_current();
        let _ = self.hide(app);
    }

    pub fn take_candidate(&self, app: &AppHandle) -> Option<CapturedSelection> {
        let _lifecycle = self
            .inner
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if !self.is_enabled() {
            return None;
        }
        self.inner.probe_sessions.cancel_current();
        let generation = self.inner.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let candidate = self
            .inner
            .candidate
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        self.inner
            .bounds
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        drop(_lifecycle);
        let _ = self.queue_native_hide(app, generation);
        candidate.filter(|selection| selection.foreground_context.is_valid_and_foreground())
    }

    fn probe_after_selection(
        &self,
        app: AppHandle,
        context: ForegroundContext,
        mouse_hint: Option<MouseSelectionHint>,
        placement_hint: Option<MouseSelectionHint>,
        epoch: u64,
    ) {
        if context.process_id == std::process::id() {
            return;
        }
        let Some(request) = self.begin_probe(epoch) else {
            return;
        };
        let manager = self.clone();
        let capture_academic_context =
            app.state::<ModelConfigStore>().get().mode == TranslationMode::Academic;
        let application_name = context.application_name.clone();
        let worker_application_name = application_name.clone();
        let spawn_result = thread::Builder::new()
            .name(format!("translay-selection-probe-{}", request.id))
            .spawn(move || {
                let started = Instant::now();
                thread::sleep(SELECTION_BUTTON_SETTLE_DELAY);
                if !manager.is_enabled_for(epoch)
                    || !manager.inner.probe_sessions.is_current(&request)
                    || !context.is_valid_and_foreground()
                {
                    return;
                }

                match SelectionService::capture_for_button(
                    context.clone(),
                    request.cancellation.clone(),
                    SELECTION_BUTTON_CAPTURE_TIMEOUT,
                    mouse_hint,
                    capture_academic_context,
                ) {
                    Ok(selection)
                        if manager.is_enabled_for(epoch)
                            && manager.inner.probe_sessions.is_current(&request)
                            && context.is_valid_and_foreground() =>
                    {
                        let lines = selection.line_rects;
                        let nearby = selection.nearby_rects;
                        let captured = CapturedSelection {
                            text: selection.text,
                            context_before: selection.context_before,
                            context_after: selection.context_after,
                            source: selection.method.to_owned(),
                            selection_rect: selection.rect,
                            foreground_context: context,
                            elapsed_ms: started.elapsed().as_millis(),
                            clipboard_restored: None,
                            warning_code: None,
                        };
                        let ui_app = app.clone();
                        let ui_manager = manager.clone();
                        let _ = app.run_on_main_thread(move || {
                            if let Err(error) = ui_manager.show_candidate(&ui_app, captured, placement_hint, &lines, nearby.as_deref(), &request, epoch) {
                                warn!(application = %worker_application_name, %error, "selection button could not be shown");
                            }
                        });
                    }
                    Ok(_) => {}
                    Err(failure) => {
                        // Automatic probing is intentionally silent. Unsupported
                        // controls still remain available through the hotkey and
                        // its clipboard fallback.
                        info!(
                            application = %worker_application_name,
                            capture_method = "selection-button",
                            text_length = 0,
                            elapsed_ms = started.elapsed().as_millis(),
                            error_code = %failure.code,
                            "automatic selection probe produced no button"
                        );
                    }
                }
            });

        if spawn_result.is_err() {
            warn!(
                application = %application_name,
                capture_method = "selection-button",
                text_length = 0,
                elapsed_ms = 0,
                error_code = "SELECTION_PROBE_START_FAILED",
                "selection probe thread could not be started"
            );
        }
    }

    fn begin_probe(&self, epoch: u64) -> Option<CaptureRequest> {
        // Keep the originating monitor epoch. Recapturing it here would allow
        // an old mouse release to become a new request after off/on toggles.
        let _lifecycle = self
            .inner
            .lifecycle
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        self.is_enabled_for(epoch)
            .then(|| self.inner.probe_sessions.begin())
    }

    fn show_candidate(
        &self,
        app: &AppHandle,
        candidate: CapturedSelection,
        placement_hint: Option<MouseSelectionHint>,
        lines: &[ScreenRect],
        nearby: Option<&[ScreenRect]>,
        request: &CaptureRequest,
        epoch: u64,
    ) -> Result<(), String> {
        if !self.is_enabled_for(epoch) || !candidate.foreground_context.is_valid_and_foreground() {
            return Ok(());
        }
        let selection_anchor = candidate.selection_rect.unwrap_or_else(cursor_anchor);
        let monitor_anchor = placement_hint
            .map(|hint| point_anchor(hint.focus))
            .unwrap_or_else(|| lines.last().copied().unwrap_or(selection_anchor));
        let (work_area, dpi) = monitor_metrics(monitor_anchor, candidate.foreground_context.hwnd)?;
        let size = scale_for_dpi(SELECTION_BUTTON_SIZE, dpi);
        let glyph_size = scale_for_dpi(SELECTION_BUTTON_GLYPH_SIZE, dpi);
        let gap = scale_for_dpi(SELECTION_BUTTON_GAP, dpi);
        let Some(position) = crate::selection_placement::place(
            lines,
            nearby,
            placement_hint,
            selection_anchor,
            work_area,
            size,
            glyph_size,
            gap,
        ) else {
            return Ok(());
        };
        let mut excluded = lines.to_vec();
        excluded.extend_from_slice(nearby.unwrap_or_default());
        excluded.retain(|rect| crate::selection_placement::intersects(*rect, position));
        let _lifecycle = self
            .inner
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if !self.is_enabled_for(epoch)
            || !candidate.foreground_context.is_valid_and_foreground()
            || !self.inner.probe_sessions.is_current(request)
        {
            return Ok(());
        }
        let hwnd = selection_button_hwnd(app)?;
        self.configure_native_style(app)?;
        // Moving a hidden window first lets WM_DPICHANGED settle on the target
        // monitor. The show below reapplies the physical size before painting.
        unsafe {
            SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                position.left,
                position.top,
                size,
                size,
                SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_HIDEWINDOW,
            )
        }
        .map_err(|error| format!("定位划词按钮失败：{error}"))?;
        clip_button_hit_region(hwnd, position, &excluded)?;
        let generation = self.inner.generation.fetch_add(1, Ordering::AcqRel) + 1;
        *self
            .inner
            .candidate
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(candidate);
        *self
            .inner
            .bounds
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(position);
        *self
            .inner
            .excluded
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = excluded.clone();
        self.emit_visibility(app, true);

        // SAFETY: coordinates are physical pixels and hwnd is the live button window.
        let show_result = unsafe {
            SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                position.left,
                position.top,
                size,
                size,
                SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_SHOWWINDOW,
            )
        }
        .map_err(|error| format!("显示划词按钮失败：{error}"));
        if let Err(error) = show_result {
            self.inner
                .candidate
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .take();
            self.inner
                .bounds
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .take();
            return Err(error);
        }

        let manager = self.clone();
        let app = app.clone();
        thread::spawn(move || {
            use crate::selection_retention::{Action, Proximity, Retention};
            let started = Instant::now();
            let mut retention = Retention::new(SELECTION_BUTTON_VISIBLE_DURATION);
            let near_gap = scale_for_dpi(12, dpi);
            while manager.inner.generation.load(Ordering::Acquire) == generation {
                let proximity = cursor_position()
                    .map(|p| {
                        if manager.contains_button(p) {
                            Proximity::Hover
                        } else if p.x >= position.left - near_gap
                            && p.x < position.right + near_gap
                            && p.y >= position.top - near_gap
                            && p.y < position.bottom + near_gap
                        {
                            Proximity::Near
                        } else {
                            Proximity::Away
                        }
                    })
                    .unwrap_or(Proximity::Away);
                match retention.tick(started.elapsed(), proximity) {
                    Action::Fade => manager.queue_visibility(&app, generation, false),
                    Action::Restore => manager.queue_visibility(&app, generation, true),
                    Action::Hide => {
                        let _ = manager.hide_generation(&app, Some(generation));
                        break;
                    }
                    Action::Keep => {}
                }
                thread::sleep(Duration::from_millis(30));
            }
        });
        Ok(())
    }

    fn hide(&self, app: &AppHandle) -> Result<(), String> {
        self.hide_generation(app, None)
    }

    fn emit_visibility(&self, app: &AppHandle, visible: bool) {
        if let Some(window) = app.get_webview_window("selection-button") {
            let revision = self
                .inner
                .visibility_revision
                .fetch_add(1, Ordering::AcqRel)
                + 1;
            let _ = window.emit(
                "selection-button-visibility",
                serde_json::json!({"revision": revision, "visible": visible}),
            );
        }
    }

    fn queue_visibility(&self, app: &AppHandle, generation: u64, visible: bool) {
        let manager = self.clone();
        let ui_app = app.clone();
        let _ = app.run_on_main_thread(move || {
            let _guard = manager
                .inner
                .lifecycle
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if manager.inner.generation.load(Ordering::Acquire) == generation {
                manager.emit_visibility(&ui_app, visible);
            }
        });
    }

    fn hide_generation(&self, app: &AppHandle, expected: Option<u64>) -> Result<(), String> {
        let _lifecycle = self
            .inner
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if expected
            .is_some_and(|generation| self.inner.generation.load(Ordering::Acquire) != generation)
        {
            return Ok(());
        }
        let generation = self.inner.generation.fetch_add(1, Ordering::AcqRel) + 1;
        self.inner
            .candidate
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        self.inner
            .bounds
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        drop(_lifecycle);
        self.queue_native_hide(app, generation)
    }

    fn queue_native_hide(&self, app: &AppHandle, generation: u64) -> Result<(), String> {
        let manager = self.clone();
        let ui_app = app.clone();
        app.run_on_main_thread(move || {
            let _lifecycle = manager
                .inner
                .lifecycle
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            // An old queued hide must not dismiss a newer selection window.
            if manager.inner.generation.load(Ordering::Acquire) == generation {
                let _ = hide_native_button(&ui_app);
            }
        })
        .map_err(|error| format!("无法收起划词按钮：{error}"))
    }

    fn contains_button(&self, point: POINT) -> bool {
        let inside = self
            .inner
            .bounds
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .is_some_and(|bounds| {
                point.x >= bounds.left
                    && point.x < bounds.right
                    && point.y >= bounds.top
                    && point.y < bounds.bottom
            });
        inside
            && !self
                .inner
                .excluded
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .any(|r| {
                    point.x >= r.left && point.x < r.right && point.y >= r.top && point.y < r.bottom
                })
    }
}

fn run_input_monitor(app: AppHandle, manager: SelectionButtonManager) {
    while manager.inner.running.load(Ordering::Acquire) {
        let mut guard = manager
            .inner
            .wake_lock
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        while manager.inner.running.load(Ordering::Acquire) && !manager.is_enabled() {
            guard = manager
                .inner
                .wake
                .wait(guard)
                .unwrap_or_else(|error| error.into_inner());
        }
        drop(guard);
        if !manager.inner.running.load(Ordering::Acquire) {
            break;
        }
        let epoch = manager.inner.monitor_epoch.load(Ordering::Acquire);
        monitor_enabled(&app, &manager, epoch);
    }
}

fn monitor_enabled(app: &AppHandle, manager: &SelectionButtonManager, epoch: u64) {
    let mut mouse_was_down = key_is_down(VK_LBUTTON.0 as i32);
    let mut mouse_press: Option<(POINT, bool)> = None;
    let mut last_release: Option<MouseRelease> = None;
    // A selection already underway when monitoring resumes is incomplete.
    let shift_down_at_start = key_is_down(VK_SHIFT.0 as i32);
    let ctrl_a_at_start = key_is_down(VK_CONTROL.0 as i32) && key_is_down(KEY_A);
    let mut keyboard_selection_in_progress = shift_down_at_start || ctrl_a_at_start;
    let mut skip_keyboard_release = keyboard_selection_in_progress;
    // The hook only counts wheel activity and always passes input through.
    // This thread pumps messages so Windows can deliver low-level callbacks.
    let wheel_hook = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(wheel_activity), None, 0) }.ok();
    if wheel_hook.is_none() {
        warn!("selection wheel-dismiss hook unavailable");
    }
    let mut scroll_epoch = SCROLL_EPOCH.load(Ordering::Acquire);
    let mut foreground = unsafe { GetForegroundWindow() };
    let mut dismiss_key_was_down = false;

    while manager.inner.running.load(Ordering::Acquire) && manager.is_enabled_for(epoch) {
        let mut message = MSG::default();
        while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        let next_scroll = SCROLL_EPOCH.load(Ordering::Acquire);
        let next_foreground = unsafe { GetForegroundWindow() };
        let dismiss_key = [
            VK_ESCAPE, VK_NEXT, VK_PRIOR, VK_UP, VK_DOWN, VK_LEFT, VK_RIGHT, VK_HOME, VK_END,
        ]
        .into_iter()
        .any(|key| key_is_down(key.0 as i32))
            && !key_is_down(VK_SHIFT.0 as i32);
        if next_scroll != scroll_epoch
            || next_foreground != foreground
            || (dismiss_key && !dismiss_key_was_down)
        {
            manager.dismiss(app);
            last_release = None;
        }
        scroll_epoch = next_scroll;
        foreground = next_foreground;
        dismiss_key_was_down = dismiss_key;
        let mouse_is_down = key_is_down(VK_LBUTTON.0 as i32);
        if mouse_is_down && !mouse_was_down {
            let point = cursor_position();
            let over_translay = point.is_some_and(|point| {
                manager.contains_button(point) || point_is_translay_window(point)
            });
            mouse_press = point.map(|point| (point, over_translay));
            if !over_translay {
                manager.dismiss(app);
            }
        } else if !mouse_is_down
            && mouse_was_down
            && let (Some((pressed, over_translay)), Some(released)) =
                (mouse_press.take(), cursor_position())
        {
            if over_translay || point_is_translay_window(released) {
                // Do not let an internal click seed an external double-click.
                last_release = None;
            } else {
                let now = Instant::now();
                let double_click_window =
                    Duration::from_millis(unsafe { GetDoubleClickTime() } as u64);
                let double_clicked = last_release.is_some_and(|previous| {
                    now.saturating_duration_since(previous.at) <= double_click_window
                        && points_are_close(previous.point, released, DRAG_THRESHOLD * 2)
                });
                let dragged = !points_are_close(pressed, released, DRAG_THRESHOLD);
                last_release = Some(MouseRelease {
                    point: released,
                    at: now,
                });
                if (dragged || double_clicked)
                    && let Ok(context) = ForegroundContext::capture()
                {
                    let mouse_hint = dragged.then_some(MouseSelectionHint {
                        anchor: pressed,
                        focus: released,
                    });
                    let placement_hint = Some(MouseSelectionHint {
                        anchor: pressed,
                        focus: released,
                    });
                    if manager.is_enabled_for(epoch) {
                        manager.probe_after_selection(
                            app.clone(),
                            context,
                            mouse_hint,
                            placement_hint,
                            epoch,
                        );
                    }
                }
            }
        }
        mouse_was_down = mouse_is_down;

        let shift_down = key_is_down(VK_SHIFT.0 as i32);
        let selection_navigation_down = [
            VK_LEFT, VK_RIGHT, VK_UP, VK_DOWN, VK_HOME, VK_END, VK_PRIOR, VK_NEXT,
        ]
        .into_iter()
        .any(|key| key_is_down(key.0 as i32));
        let ctrl_a_down = key_is_down(VK_CONTROL.0 as i32) && key_is_down(KEY_A);
        let keyboard_selection_down = (shift_down && selection_navigation_down) || ctrl_a_down;
        if keyboard_selection_down && !keyboard_selection_in_progress {
            keyboard_selection_in_progress = true;
            manager.dismiss(app);
        } else if keyboard_selection_in_progress && !shift_down && !ctrl_a_down {
            keyboard_selection_in_progress = false;
            if !skip_keyboard_release
                && manager.is_enabled_for(epoch)
                && let Ok(context) = ForegroundContext::capture()
            {
                manager.probe_after_selection(app.clone(), context, None, None, epoch);
            }
            skip_keyboard_release = false;
        }

        // Wake immediately for hook messages instead of delaying mouse input
        // behind the keyboard-poll interval.
        unsafe {
            MsgWaitForMultipleObjectsEx(
                None,
                INPUT_POLL_INTERVAL.as_millis() as u32,
                QS_ALLINPUT,
                MWMO_INPUTAVAILABLE,
            );
        }
    }
    if let Some(hook) = wheel_hook {
        let _ = unsafe { UnhookWindowsHookEx(hook) };
    }
}

unsafe extern "system" fn wheel_activity(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && matches!(wparam.0 as u32, WM_MOUSEWHEEL | WM_MOUSEHWHEEL) {
        SCROLL_EPOCH.fetch_add(1, Ordering::AcqRel);
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

fn clip_button_hit_region(
    hwnd: HWND,
    bounds: ScreenRect,
    excluded: &[ScreenRect],
) -> Result<(), String> {
    // Windows owns the region only after a successful SetWindowRgn. Each
    // temporary subtraction region is deleted on every path.
    unsafe {
        let region = CreateRectRgn(0, 0, bounds.width(), bounds.height());
        if region.0.is_null() {
            return Err("无法准备划词按钮点击区域".to_owned());
        }
        for rect in excluded {
            let cut = CreateRectRgn(
                rect.left - bounds.left,
                rect.top - bounds.top,
                rect.right - bounds.left,
                rect.bottom - bounds.top,
            );
            if cut.0.is_null() {
                let _ = DeleteObject(region.into());
                return Err("无法裁剪划词按钮点击区域".to_owned());
            }
            let result = CombineRgn(Some(region), Some(region), Some(cut), RGN_DIFF);
            let _ = DeleteObject(cut.into());
            if result == RGN_ERROR {
                let _ = DeleteObject(region.into());
                return Err("无法裁剪划词按钮点击区域".to_owned());
            }
        }
        if SetWindowRgn(hwnd, Some(region), false) == 0 {
            let _ = DeleteObject(region.into());
            return Err("无法应用划词按钮点击区域".to_owned());
        }
    }
    Ok(())
}

// Non-activating overlays leave the source app foreground. Inspect the actual
// hit window and its root, including WebView child windows in another process.
fn point_is_translay_window(point: POINT) -> bool {
    window_is_translay(unsafe { WindowFromPoint(point) })
}

fn window_is_translay(hit: HWND) -> bool {
    unsafe {
        let root = GetAncestor(hit, GA_ROOT);
        let mut process_id = 0;
        GetWindowThreadProcessId(root, Some(&mut process_id));
        process_id == std::process::id()
    }
}

fn key_is_down(key: i32) -> bool {
    // SAFETY: GetAsyncKeyState accepts all virtual-key values.
    unsafe { GetAsyncKeyState(key) as u16 & 0x8000 != 0 }
}

fn cursor_position() -> Option<POINT> {
    let mut point = POINT::default();
    // SAFETY: point is a valid output pointer.
    unsafe { GetCursorPos(&mut point) }.ok().map(|_| point)
}

fn points_are_close(first: POINT, second: POINT, threshold: i32) -> bool {
    first.x.abs_diff(second.x) <= threshold.max(0) as u32
        && first.y.abs_diff(second.y) <= threshold.max(0) as u32
}

fn point_anchor(point: POINT) -> ScreenRect {
    ScreenRect {
        left: point.x,
        top: point.y,
        right: point.x.saturating_add(1),
        bottom: point.y.saturating_add(1),
    }
}

fn selection_button_hwnd(app: &AppHandle) -> Result<HWND, String> {
    app.get_webview_window("selection-button")
        .ok_or_else(|| "找不到预创建的划词按钮窗口".to_owned())?
        .hwnd()
        .map_err(|error| error.to_string())
}

fn hide_native_button(app: &AppHandle) -> Result<(), String> {
    let hwnd = selection_button_hwnd(app)?;
    // SAFETY: hwnd is a live Tauri window and hiding it does not activate it.
    unsafe {
        SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_HIDEWINDOW,
        )
        .map_err(|error| format!("隐藏划词按钮失败：{error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_monitor_cannot_submit_or_cancel_new_probe_after_reenable() {
        let manager = SelectionButtonManager::default();
        let old_epoch = manager.inner.monitor_epoch.load(Ordering::Acquire);
        let old_request = manager.begin_probe(old_epoch).unwrap();
        manager.transition_enabled(false);
        assert!(old_request.cancellation.is_cancelled());
        assert!(manager.begin_probe(old_epoch).is_none());
        manager.transition_enabled(true);
        let current_epoch = manager.inner.monitor_epoch.load(Ordering::Acquire);
        let current_request = manager.begin_probe(current_epoch).unwrap();
        assert!(manager.begin_probe(old_epoch).is_none());
        assert!(manager.inner.probe_sessions.is_current(&current_request));
    }

    #[test]
    fn disabling_clears_candidate_geometry_and_pending_capture() {
        let manager = SelectionButtonManager::default();
        *manager.inner.candidate.lock().unwrap() = Some(CapturedSelection {
            text: "previous selection".into(),
            context_before: None,
            context_after: None,
            source: "test".into(),
            selection_rect: None,
            foreground_context: ForegroundContext {
                hwnd: HWND::default(),
                process_id: 0,
                application_name: "test".into(),
            },
            elapsed_ms: 0,
            clipboard_restored: None,
            warning_code: None,
        });
        *manager.inner.bounds.lock().unwrap() = Some(ScreenRect {
            left: 10,
            top: 10,
            right: 50,
            bottom: 50,
        });
        manager
            .inner
            .excluded
            .lock()
            .unwrap()
            .push(ScreenRect::default());
        manager.transition_enabled(false);
        assert!(manager.inner.candidate.lock().unwrap().is_none());
        assert!(manager.inner.bounds.lock().unwrap().is_none());
        assert!(manager.inner.excluded.lock().unwrap().is_empty());
        manager.transition_enabled(true);
        assert!(manager.inner.candidate.lock().unwrap().is_none());
    }

    #[test]
    fn rapid_toggle_invalidates_old_probes_and_keeps_only_the_last_epoch() {
        let manager = SelectionButtonManager::default();
        let old_epoch = manager.inner.monitor_epoch.load(Ordering::Acquire);
        let old_probe = manager.inner.probe_sessions.begin();
        assert!(manager.transition_enabled(false));
        assert!(!manager.is_enabled());
        assert!(!manager.inner.probe_sessions.is_current(&old_probe));
        assert!(!manager.is_enabled_for(old_epoch));
        assert!(manager.transition_enabled(true));
        let current_epoch = manager.inner.monitor_epoch.load(Ordering::Acquire);
        assert!(manager.is_enabled_for(current_epoch));
        assert!(!manager.is_enabled_for(old_epoch));
        assert!(!manager.transition_enabled(true));
        assert_eq!(
            manager.inner.monitor_epoch.load(Ordering::Acquire),
            current_epoch
        );
        assert!(manager.transition_enabled(false));
        assert!(!manager.is_enabled_for(current_epoch));
    }

    #[test]
    fn wheel_listener_can_be_installed_and_removed_without_input_injection() {
        unsafe {
            let hook = SetWindowsHookExW(WH_MOUSE_LL, Some(wheel_activity), None, 0).unwrap();
            UnhookWindowsHookEx(hook).unwrap();
        }
    }

    #[test]
    fn native_region_and_monitor_hit_test_leave_source_text_clickable() {
        use windows::{
            Win32::{
                Graphics::Gdi::{GetWindowRgn, PtInRegion},
                UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WS_POPUP},
            },
            core::w,
        };
        unsafe {
            let hwnd = CreateWindowExW(
                Default::default(),
                w!("STATIC"),
                w!("selection-region-test"),
                WS_POPUP,
                100,
                100,
                40,
                40,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            let bounds = ScreenRect {
                left: 100,
                top: 100,
                right: 140,
                bottom: 140,
            };
            let text = ScreenRect {
                left: 80,
                top: 110,
                right: 105,
                bottom: 130,
            };
            clip_button_hit_region(hwnd, bounds, &[text]).unwrap();
            let region = CreateRectRgn(0, 0, 0, 0);
            assert_ne!(GetWindowRgn(hwnd, region), RGN_ERROR);
            assert!(!PtInRegion(region, 3, 20).as_bool());
            assert!(PtInRegion(region, 20, 20).as_bool());
            // A subsequent selection must not inherit the previous cutout.
            clip_button_hit_region(hwnd, bounds, &[]).unwrap();
            GetWindowRgn(hwnd, region);
            assert!(PtInRegion(region, 3, 20).as_bool());
            let _ = DeleteObject(region.into());
            DestroyWindow(hwnd).unwrap();
            let manager = SelectionButtonManager::default();
            *manager.inner.bounds.lock().unwrap() = Some(bounds);
            *manager.inner.excluded.lock().unwrap() = vec![text];
            assert!(!manager.contains_button(POINT { x: 103, y: 120 }));
            assert!(manager.contains_button(POINT { x: 120, y: 120 }));
        }
    }

    #[test]
    fn own_root_and_webview_style_child_are_excluded_without_activation() {
        use windows::{
            Win32::UI::WindowsAndMessaging::{
                CreateWindowExW, DestroyWindow, GetDesktopWindow, WS_CHILD, WS_POPUP,
            },
            core::w,
        };
        // Hidden native windows exercise root ownership without changing focus.
        unsafe {
            let root = CreateWindowExW(
                Default::default(),
                w!("STATIC"),
                w!("selection-test"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            let child = CreateWindowExW(
                Default::default(),
                w!("STATIC"),
                w!("child"),
                WS_CHILD,
                0,
                0,
                1,
                1,
                Some(root),
                None,
                None,
                None,
            )
            .unwrap();
            let root_is_ours = window_is_translay(root);
            let child_is_ours = window_is_translay(child);
            let desktop_is_ours = window_is_translay(GetDesktopWindow());
            DestroyWindow(root).unwrap();
            assert!(root_is_ours);
            assert!(child_is_ours);
            assert!(!desktop_is_ours);
            assert!(!window_is_translay(HWND::default()));
        }
    }

    #[test]
    fn drag_threshold_filters_stationary_clicks() {
        let origin = POINT { x: 100, y: 200 };
        assert!(points_are_close(origin, POINT { x: 104, y: 196 }, 4));
        assert!(!points_are_close(origin, POINT { x: 105, y: 200 }, 4));
    }
}
