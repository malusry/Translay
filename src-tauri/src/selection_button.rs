use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use tauri::{AppHandle, Manager};
use tracing::{info, warn};
use windows::Win32::{
    Foundation::{HWND, POINT},
    UI::{
        Input::KeyboardAndMouse::{
            GetAsyncKeyState, GetDoubleClickTime, VK_CONTROL, VK_DOWN, VK_END, VK_HOME, VK_LBUTTON,
            VK_LEFT, VK_NEXT, VK_PRIOR, VK_RIGHT, VK_SHIFT, VK_UP,
        },
        WindowsAndMessaging::{
            GWL_EXSTYLE, GetCursorPos, GetWindowLongPtrW, HWND_TOPMOST, SWP_ASYNCWINDOWPOS,
            SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_SHOWWINDOW, SetWindowLongPtrW,
            SetWindowPos, WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
        },
    },
};

use crate::{
    capture_session::CaptureSession,
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

#[derive(Clone)]
pub struct SelectionButtonManager {
    inner: Arc<SelectionButtonState>,
}

struct SelectionButtonState {
    candidate: Mutex<Option<CapturedSelection>>,
    bounds: Mutex<Option<ScreenRect>>,
    lifecycle: Mutex<()>,
    probe_sessions: CaptureSession,
    generation: AtomicU64,
    running: AtomicBool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ButtonPlacement {
    x: i32,
    y: i32,
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
                lifecycle: Mutex::new(()),
                probe_sessions: CaptureSession::default(),
                generation: AtomicU64::new(0),
                running: AtomicBool::new(false),
            }),
        }
    }
}

impl SelectionButtonManager {
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
        self.inner.running.store(false, Ordering::Release);
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
        self.inner.generation.fetch_add(1, Ordering::AcqRel);
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
        let _ = hide_native_button(app);
        candidate.filter(|selection| selection.foreground_context.is_valid_and_foreground())
    }

    fn probe_after_selection(
        &self,
        app: AppHandle,
        context: ForegroundContext,
        mouse_hint: Option<MouseSelectionHint>,
        placement_hint: Option<MouseSelectionHint>,
    ) {
        if context.process_id == std::process::id() {
            return;
        }
        let request = self.inner.probe_sessions.begin();
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
                if !manager.inner.probe_sessions.is_current(&request)
                    || !context.is_valid_and_foreground()
                {
                    return;
                }

                match SelectionService::capture_with_timeout(
                    context.clone(),
                    request.cancellation.clone(),
                    SELECTION_BUTTON_CAPTURE_TIMEOUT,
                    mouse_hint,
                    capture_academic_context,
                ) {
                    Ok(selection)
                        if manager.inner.probe_sessions.is_current(&request)
                            && context.is_valid_and_foreground() =>
                    {
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
                        if let Err(error) = manager.show_candidate(&app, captured, placement_hint) {
                            warn!(
                                application = %worker_application_name,
                                capture_method = "selection-button",
                                text_length = 0,
                                elapsed_ms = started.elapsed().as_millis(),
                                error_code = "SELECTION_BUTTON_SHOW_FAILED",
                                %error,
                                "selection button could not be shown"
                            );
                        }
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

    fn show_candidate(
        &self,
        app: &AppHandle,
        candidate: CapturedSelection,
        placement_hint: Option<MouseSelectionHint>,
    ) -> Result<(), String> {
        if !candidate.foreground_context.is_valid_and_foreground() {
            return Ok(());
        }
        let selection_anchor = candidate.selection_rect.unwrap_or_else(cursor_anchor);
        let monitor_anchor = placement_hint
            .map(|hint| point_anchor(hint.focus))
            .unwrap_or(selection_anchor);
        let (work_area, dpi) = monitor_metrics(monitor_anchor, candidate.foreground_context.hwnd)?;
        let size = scale_for_dpi(SELECTION_BUTTON_SIZE, dpi);
        let glyph_size = scale_for_dpi(SELECTION_BUTTON_GLYPH_SIZE, dpi);
        let gap = scale_for_dpi(SELECTION_BUTTON_GAP, dpi);
        let position = match (placement_hint, candidate.selection_rect) {
            (Some(hint), Some(selection)) => calculate_selection_side_button_position(
                selection, hint.focus, work_area, size, glyph_size, gap,
            ),
            (Some(hint), None) => {
                calculate_pointer_button_position(hint, work_area, size, glyph_size, gap)
            }
            (None, _) => {
                calculate_button_position(selection_anchor, work_area, size, glyph_size, gap)
            }
        };
        let _lifecycle = self
            .inner
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if !candidate.foreground_context.is_valid_and_foreground() {
            return Ok(());
        }
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
            .unwrap_or_else(|error| error.into_inner()) = Some(ScreenRect {
            left: position.x,
            top: position.y,
            right: position.x.saturating_add(size),
            bottom: position.y.saturating_add(size),
        });

        let hwnd = selection_button_hwnd(app)?;
        self.configure_native_style(app)?;
        // SAFETY: coordinates are physical pixels and hwnd is the live button window.
        let show_result = unsafe {
            SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                position.x,
                position.y,
                size,
                size,
                SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_ASYNCWINDOWPOS | SWP_SHOWWINDOW,
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
            thread::sleep(SELECTION_BUTTON_VISIBLE_DURATION);
            if manager.inner.generation.load(Ordering::Acquire) == generation {
                let _ = manager.hide(&app);
            }
        });
        Ok(())
    }

    fn hide(&self, app: &AppHandle) -> Result<(), String> {
        let _lifecycle = self
            .inner
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
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
        hide_native_button(app)
    }

    fn contains_button(&self, point: POINT) -> bool {
        self.inner
            .bounds
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .is_some_and(|bounds| {
                point.x >= bounds.left
                    && point.x < bounds.right
                    && point.y >= bounds.top
                    && point.y < bounds.bottom
            })
    }
}

fn run_input_monitor(app: AppHandle, manager: SelectionButtonManager) {
    let mut mouse_was_down = key_is_down(VK_LBUTTON.0 as i32);
    let mut mouse_press: Option<(POINT, bool)> = None;
    let mut last_release: Option<MouseRelease> = None;
    let mut keyboard_selection_in_progress = false;

    while manager.inner.running.load(Ordering::Acquire) {
        let mouse_is_down = key_is_down(VK_LBUTTON.0 as i32);
        if mouse_is_down && !mouse_was_down {
            let point = cursor_position();
            let over_translay = point.is_some_and(|point| manager.contains_button(point));
            mouse_press = point.map(|point| (point, over_translay));
            if !over_translay {
                manager.dismiss(&app);
            }
        } else if !mouse_is_down
            && mouse_was_down
            && let (Some((pressed, over_translay)), Some(released)) =
                (mouse_press.take(), cursor_position())
        {
            if !over_translay {
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
                    manager.probe_after_selection(app.clone(), context, mouse_hint, placement_hint);
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
            manager.dismiss(&app);
        } else if keyboard_selection_in_progress && !shift_down && !ctrl_a_down {
            keyboard_selection_in_progress = false;
            if let Ok(context) = ForegroundContext::capture() {
                manager.probe_after_selection(app.clone(), context, None, None);
            }
        }

        thread::sleep(INPUT_POLL_INTERVAL);
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

fn calculate_pointer_button_position(
    hint: MouseSelectionHint,
    work_area: ScreenRect,
    size: i32,
    glyph_size: i32,
    gap: i32,
) -> ButtonPlacement {
    let max_x = (work_area.right - size).max(work_area.left);
    let max_y = (work_area.bottom - size).max(work_area.top);
    let glyph_size = glyph_size.clamp(0, size);
    let inset = size.saturating_sub(glyph_size) / 2;
    let focus = hint.focus;
    let centered_x = focus.x.saturating_sub(size / 2);
    let centered_y = focus.y.saturating_sub(size / 2);
    let right_x = focus.x.saturating_add(gap).saturating_sub(inset);
    let left_x = focus
        .x
        .saturating_sub(gap)
        .saturating_sub(glyph_size)
        .saturating_sub(inset);
    let below_y = focus.y.saturating_add(gap).saturating_sub(inset);
    let above_y = focus
        .y
        .saturating_sub(gap)
        .saturating_sub(glyph_size)
        .saturating_sub(inset);
    let forward = focus.y > hint.anchor.y || (focus.y == hint.anchor.y && focus.x >= hint.anchor.x);
    let side_candidates = if forward {
        [(right_x, centered_y), (left_x, centered_y)]
    } else {
        [(left_x, centered_y), (right_x, centered_y)]
    };
    let candidates = [
        side_candidates[0],
        (centered_x, below_y),
        (centered_x, above_y),
        side_candidates[1],
    ];

    for (x, y) in candidates {
        if x >= work_area.left
            && y >= work_area.top
            && x.saturating_add(size) <= work_area.right
            && y.saturating_add(size) <= work_area.bottom
        {
            return ButtonPlacement { x, y };
        }
    }

    ButtonPlacement {
        x: side_candidates[0].0.clamp(work_area.left, max_x),
        y: centered_y.clamp(work_area.top, max_y),
    }
}

fn calculate_selection_side_button_position(
    selection: ScreenRect,
    focus: POINT,
    work_area: ScreenRect,
    size: i32,
    glyph_size: i32,
    gap: i32,
) -> ButtonPlacement {
    // Some providers return a selection rectangle that stops before the mouse
    // focus. Expand it to the observed endpoint so the button cannot be placed
    // back over the selected text in those applications.
    let selection = ScreenRect {
        left: selection.left.min(focus.x),
        top: selection.top.min(focus.y),
        right: selection.right.max(focus.x.saturating_add(1)),
        bottom: selection.bottom.max(focus.y.saturating_add(1)),
    };
    let distance_to_left = (i64::from(focus.x) - i64::from(selection.left)).abs();
    let distance_to_right = (i64::from(focus.x) - i64::from(selection.right)).abs();
    calculate_button_position_with_side(
        selection,
        distance_to_right <= distance_to_left,
        work_area,
        size,
        glyph_size,
        gap,
    )
}

fn calculate_button_position(
    selection: ScreenRect,
    work_area: ScreenRect,
    size: i32,
    glyph_size: i32,
    gap: i32,
) -> ButtonPlacement {
    calculate_button_position_with_side(selection, true, work_area, size, glyph_size, gap)
}

fn calculate_button_position_with_side(
    selection: ScreenRect,
    prefer_right: bool,
    work_area: ScreenRect,
    size: i32,
    glyph_size: i32,
    gap: i32,
) -> ButtonPlacement {
    let max_x = (work_area.right - size).max(work_area.left);
    let max_y = (work_area.bottom - size).max(work_area.top);
    let glyph_size = glyph_size.clamp(0, size);
    let inset = size.saturating_sub(glyph_size) / 2;
    let centered_y = selection.top.saturating_add(
        selection
            .bottom
            .saturating_sub(selection.top)
            .saturating_sub(size)
            / 2,
    );
    let align_right_x = selection
        .right
        .saturating_sub(inset.saturating_add(glyph_size));
    // Position both sides from the visible glyph edge, not the transparent
    // click target, so left and right have the same optical separation.
    let right = (
        selection.right.saturating_add(gap).saturating_sub(inset),
        centered_y,
    );
    let left = (
        selection
            .left
            .saturating_sub(gap)
            .saturating_sub(inset.saturating_add(glyph_size)),
        centered_y,
    );
    let side_candidates = if prefer_right {
        [right, left]
    } else {
        [left, right]
    };
    let candidates = [
        side_candidates[0],
        side_candidates[1],
        (
            align_right_x,
            selection.bottom.saturating_add(gap).saturating_sub(inset),
        ),
        (
            align_right_x,
            selection
                .top
                .saturating_sub(gap)
                .saturating_sub(inset.saturating_add(glyph_size)),
        ),
    ];

    for (x, y) in candidates {
        if x >= work_area.left
            && y >= work_area.top
            && x.saturating_add(size) <= work_area.right
            && y.saturating_add(size) <= work_area.bottom
        {
            return ButtonPlacement { x, y };
        }
    }

    ButtonPlacement {
        x: side_candidates[0].0.clamp(work_area.left, max_x),
        y: centered_y.clamp(work_area.top, max_y),
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
            SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_ASYNCWINDOWPOS | SWP_HIDEWINDOW,
        )
        .map_err(|error| format!("隐藏划词按钮失败：{error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK_AREA: ScreenRect = ScreenRect {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };

    #[test]
    fn button_prefers_the_centered_right_side_of_the_selection() {
        let selection = ScreenRect {
            left: 200,
            top: 100,
            right: 520,
            bottom: 140,
        };
        assert_eq!(
            calculate_button_position(selection, WORK_AREA, 40, 20, 3),
            ButtonPlacement { x: 513, y: 100 }
        );
    }

    #[test]
    fn button_flips_inside_the_monitor_at_the_right_edge() {
        let selection = ScreenRect {
            left: 1720,
            top: 900,
            right: 1915,
            bottom: 940,
        };
        let placement = calculate_button_position(selection, WORK_AREA, 40, 20, 3);
        assert!(placement.x >= WORK_AREA.left);
        assert!(placement.x + 40 <= WORK_AREA.right);
        assert!(placement.y >= WORK_AREA.top);
        assert!(placement.y + 40 <= WORK_AREA.bottom);
        assert_eq!(placement.x + 10 + 20, selection.left - 3);
    }

    #[test]
    fn visible_glyph_stays_close_without_covering_the_selection() {
        let selection = ScreenRect {
            left: 100,
            top: 200,
            right: 360,
            bottom: 224,
        };

        let placement = calculate_button_position(selection, WORK_AREA, 40, 20, 3);
        let glyph_left = placement.x + 10;
        let glyph_center_y = placement.y + 20;

        assert_eq!(glyph_left, selection.right + 3);
        assert_eq!(glyph_center_y, selection.top + 12);
    }

    #[test]
    fn pointer_placement_follows_the_mouse_release_instead_of_a_large_provider_rect() {
        let hint = MouseSelectionHint {
            anchor: POINT { x: 120, y: 120 },
            focus: POINT { x: 480, y: 700 },
        };

        let placement = calculate_pointer_button_position(hint, WORK_AREA, 40, 20, 3);
        let glyph_left = placement.x + 10;
        let glyph_center_y = placement.y + 20;

        assert_eq!(glyph_left, hint.focus.x + 3);
        assert_eq!(glyph_center_y, hint.focus.y);
    }

    #[test]
    fn pointer_placement_uses_the_leading_side_for_a_backward_selection() {
        let hint = MouseSelectionHint {
            anchor: POINT { x: 500, y: 400 },
            focus: POINT { x: 300, y: 400 },
        };

        let placement = calculate_pointer_button_position(hint, WORK_AREA, 40, 20, 3);
        let glyph_right = placement.x + 10 + 20;

        assert_eq!(glyph_right, hint.focus.x - 3);
    }

    #[test]
    fn pointer_placement_flips_inside_the_monitor_at_an_edge() {
        let hint = MouseSelectionHint {
            anchor: POINT { x: 1800, y: 500 },
            focus: POINT { x: 1915, y: 500 },
        };

        let placement = calculate_pointer_button_position(hint, WORK_AREA, 40, 20, 3);

        assert!(placement.x >= WORK_AREA.left);
        assert!(placement.x + 40 <= WORK_AREA.right);
        assert!(placement.y >= WORK_AREA.top);
        assert!(placement.y + 40 <= WORK_AREA.bottom);
    }

    #[test]
    fn mouse_near_the_left_edge_places_the_glyph_outside_the_left_side() {
        let selected_word = ScreenRect {
            left: 998,
            top: 883,
            right: 1117,
            bottom: 939,
        };
        let focus = POINT { x: 1054, y: 890 };
        let placement =
            calculate_selection_side_button_position(selected_word, focus, WORK_AREA, 40, 20, 3);
        let glyph_right = placement.x + 10 + 20;
        let glyph_center_y = placement.y + 20;

        assert_eq!(glyph_right, selected_word.left - 3);
        assert_eq!(
            glyph_center_y,
            selected_word.top + selected_word.height() / 2
        );
    }

    #[test]
    fn mouse_near_the_right_edge_places_the_glyph_outside_the_right_side() {
        let selection = ScreenRect {
            left: 300,
            top: 400,
            right: 700,
            bottom: 440,
        };
        let focus = POINT { x: 680, y: 420 };
        let placement =
            calculate_selection_side_button_position(selection, focus, WORK_AREA, 40, 20, 3);
        let glyph_left = placement.x + 10;

        assert_eq!(glyph_left, selection.right + 3);
    }

    #[test]
    fn right_side_uses_the_mouse_endpoint_when_provider_bounds_are_too_short() {
        let selection = ScreenRect {
            left: 300,
            top: 400,
            right: 650,
            bottom: 440,
        };
        let focus = POINT { x: 680, y: 420 };

        let placement =
            calculate_selection_side_button_position(selection, focus, WORK_AREA, 40, 20, 3);
        let glyph_left = placement.x + 10;

        assert_eq!(glyph_left, focus.x + 1 + 3);
    }

    #[test]
    fn drag_threshold_filters_stationary_clicks() {
        let origin = POINT { x: 100, y: 200 };
        assert!(points_are_close(origin, POINT { x: 104, y: 196 }, 4));
        assert!(!points_are_close(origin, POINT { x: 105, y: 200 }, 4));
    }
}
