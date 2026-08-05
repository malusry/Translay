use std::{
    mem::size_of,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use tauri::{AppHandle, Emitter, Manager};
use tracing::warn;
use windows::Win32::{
    Foundation::{HWND, RECT},
    Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromRect},
    UI::{
        HiDpi::{GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI},
        WindowsAndMessaging::{
            GWL_EXSTYLE, GetCursorPos, GetWindowLongPtrW, GetWindowRect, HWND_TOPMOST,
            IsWindowVisible, SWP_ASYNCWINDOWPOS, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOOWNERZORDER,
            SWP_SHOWWINDOW, SetWindowLongPtrW, SetWindowPos, WS_EX_APPWINDOW, WS_EX_NOACTIVATE,
            WS_EX_TOOLWINDOW,
        },
    },
};

use crate::{
    config::{
        OVERLAY_DELIVERY_TIMEOUT, OVERLAY_DISMISS_COMPLETION_TIMEOUT, OVERLAY_GAP,
        OVERLAY_MIN_WIDTH,
    },
    foreground_context::ForegroundContext,
    latest_capture_store::{AckStatus, LatestCaptureStore},
    models::{CapturePayload, ScreenRect},
    overlay_policy::{
        DismissalClock, DismissalHandshakeState, auto_hide_delay, calculate_overlay_position,
        dismissal_handshake_state, overlay_logical_size, scale_for_dpi,
    },
};

#[derive(Clone, Copy, Debug)]
struct OverlayLayoutContext {
    anchor: ScreenRect,
    work_area: ScreenRect,
    dpi: u32,
}

#[derive(Clone, Default)]
pub struct OverlayManager {
    visible: Arc<AtomicBool>,
    shown_request: Arc<AtomicU64>,
    hovered: Arc<AtomicBool>,
    lifecycle: Arc<Mutex<()>>,
    layout: Arc<Mutex<Option<OverlayLayoutContext>>>,
    latest_capture: LatestCaptureStore,
}

impl OverlayManager {
    pub fn new(latest_capture: LatestCaptureStore) -> Self {
        Self {
            visible: Arc::new(AtomicBool::new(false)),
            shown_request: Arc::new(AtomicU64::new(0)),
            hovered: Arc::new(AtomicBool::new(false)),
            lifecycle: Arc::new(Mutex::new(())),
            layout: Arc::new(Mutex::new(None)),
            latest_capture,
        }
    }

    pub fn begin_request(&self, request_id: u64) {
        self.hovered.store(false, Ordering::Release);
        self.latest_capture.begin_request(request_id);
    }

    pub fn configure_native_style(&self, app: &AppHandle) -> Result<(), String> {
        let hwnd = overlay_hwnd(app)?;
        // SAFETY: hwnd is a live Tauri top-level window. Updating GWL_EXSTYLE is
        // followed by SetWindowPos when shown, which applies the style change.
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
                return Err("Win32 扩展样式复核失败：浮层可能会激活或出现在任务栏".to_owned());
            }
        }
        Ok(())
    }

    pub fn show(
        &self,
        app: &AppHandle,
        payload: &CapturePayload,
        original: &ForegroundContext,
        anchor: ScreenRect,
    ) -> Result<bool, String> {
        let _lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        // This is intentionally the first side effect of show. If the WebView
        // has not mounted yet, get_latest_capture can recover this payload.
        if !self.latest_capture.store(payload.clone()) {
            return Err("拒绝显示已经过期的捕获结果".to_owned());
        }
        let window = app
            .get_webview_window("overlay")
            .ok_or_else(|| "找不到预创建的 overlay 窗口".to_owned())?;
        let hwnd = window.hwnd().map_err(|error| error.to_string())?;
        self.configure_native_style(app)?;

        let (work_area, dpi) = monitor_metrics(anchor, original.hwnd)?;
        let layout = OverlayLayoutContext {
            anchor,
            work_area,
            dpi,
        };
        *self
            .layout
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(layout);

        // SAFETY: all coordinates are physical screen pixels. SWP_NOACTIVATE and
        // WS_EX_NOACTIVATE prevent focus transfer; HWND_TOPMOST keeps the tool
        // window above ordinary application windows.
        apply_overlay_layout(hwnd, payload, layout)?;
        if !wait_for_visibility(hwnd, true, Duration::from_millis(120)) {
            self.latest_capture.clear_if_request(payload.request_id);
            return Err("浮层未在预期时间内进入可见状态".to_owned());
        }

        self.visible.store(true, Ordering::Release);
        self.shown_request
            .store(payload.request_id, Ordering::Release);
        if !self.latest_capture.mark_shown(payload.request_id) {
            self.latest_capture.clear_if_request(payload.request_id);
            return Err("捕获结果在显示前已被更新请求取代".to_owned());
        }

        // Keep the event fast path, but only after native visibility is
        // confirmed. The managed store remains the lossless recovery path.
        let emit_result = window
            .emit("capture-result", payload)
            .map_err(|error| error.to_string());
        let focus_preserved = original.is_still_foreground();
        self.schedule_delivery_watchdog(app.clone(), payload.request_id);
        emit_result?;
        Ok(focus_preserved)
    }

    pub fn acknowledge(&self, app: &AppHandle, request_id: u64) -> bool {
        if self.shown_request.load(Ordering::Acquire) != request_id {
            return false;
        }
        let terminal_payload = self
            .latest_capture
            .get()
            .filter(|payload| payload.request_id == request_id)
            .filter(|payload| payload.phase.is_terminal());
        match self.latest_capture.acknowledge(request_id) {
            AckStatus::Accepted => {
                if let Some(payload) = terminal_payload {
                    self.schedule_acknowledged_hide(
                        app.clone(),
                        request_id,
                        auto_hide_delay(&payload),
                    );
                }
                true
            }
            AckStatus::AlreadyAcknowledged => true,
            AckStatus::Rejected => false,
        }
    }

    pub fn update(&self, app: &AppHandle, payload: &CapturePayload) -> Result<(), String> {
        let _lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if !self.visible.load(Ordering::Acquire)
            || self.shown_request.load(Ordering::Acquire) != payload.request_id
        {
            return Err("浮层已被更新请求取代或隐藏".to_owned());
        }
        if !self.latest_capture.store(payload.clone())
            || !self.latest_capture.mark_shown(payload.request_id)
        {
            return Err("翻译结果已被更新请求取代".to_owned());
        }
        let window = app
            .get_webview_window("overlay")
            .ok_or_else(|| "找不到预创建的 overlay 窗口".to_owned())?;
        let layout = *self
            .layout
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(layout) = layout {
            let hwnd = window.hwnd().map_err(|error| error.to_string())?;
            apply_overlay_layout(hwnd, payload, layout)?;
        }
        window
            .emit("capture-result", payload)
            .map_err(|error| error.to_string())?;
        self.schedule_delivery_watchdog(app.clone(), payload.request_id);
        Ok(())
    }

    pub fn hide(&self, app: &AppHandle) -> Result<(), String> {
        let _lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let request_id = self.shown_request.load(Ordering::Acquire);
        self.hide_locked(app, request_id)
    }

    pub fn dismiss_request(&self, app: &AppHandle, request_id: u64) -> bool {
        let _lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if self.shown_request.load(Ordering::Acquire) != request_id {
            return false;
        }
        self.hide_locked(app, request_id).is_ok()
    }

    pub fn set_hovered(&self, request_id: u64, hovered: bool) -> bool {
        if self.shown_request.load(Ordering::Acquire) != request_id {
            return false;
        }
        self.hovered.store(hovered, Ordering::Release);
        true
    }

    pub fn fit_height(&self, app: &AppHandle, request_id: u64, logical_height: i32) -> bool {
        let _lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if self.shown_request.load(Ordering::Acquire) != request_id {
            return false;
        }
        let Some(payload) = self
            .latest_capture
            .get()
            .filter(|payload| payload.request_id == request_id)
        else {
            return false;
        };
        let Some(layout) = *self
            .layout
            .lock()
            .unwrap_or_else(|error| error.into_inner())
        else {
            return false;
        };
        let Ok(hwnd) = overlay_hwnd(app) else {
            return false;
        };
        apply_overlay_layout_with_height(
            hwnd,
            &payload,
            layout,
            Some(logical_height.clamp(60, 320)),
        )
        .is_ok()
    }

    fn hide_if_acknowledged(&self, app: &AppHandle, request_id: u64) -> Result<bool, String> {
        let _lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        // This timer is created only by the first accepted ACK. Checking the
        // native shown generation is sufficient and lets an acknowledged old
        // overlay hide while a slower new capture is still in progress.
        if self.shown_request.load(Ordering::Acquire) != request_id {
            return Ok(false);
        }
        self.hide_locked(app, request_id)?;
        Ok(true)
    }

    fn hide_if_unacknowledged(&self, app: &AppHandle, request_id: u64) -> Result<bool, String> {
        let _lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if self.shown_request.load(Ordering::Acquire) != request_id
            || !self.latest_capture.is_current_unacknowledged(request_id)
        {
            return Ok(false);
        }
        self.hide_locked(app, request_id)?;
        Ok(true)
    }

    fn hide_locked(&self, app: &AppHandle, request_id: u64) -> Result<(), String> {
        let hwnd = overlay_hwnd(app)?;
        // SAFETY: hwnd is a live Tauri window. Hiding does not activate any window.
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
            .map_err(|error| format!("隐藏浮层失败：{error}"))?;
        }
        if !wait_for_visibility(hwnd, false, Duration::from_millis(120)) {
            return Err("浮层未在预期时间内完成隐藏".to_owned());
        }
        self.finish_hide(request_id);
        Ok(())
    }

    fn finish_hide(&self, request_id: u64) {
        self.visible.store(false, Ordering::Release);
        self.shown_request.store(0, Ordering::Release);
        self.hovered.store(false, Ordering::Release);
        *self
            .layout
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = None;
        self.latest_capture.clear_if_request(request_id);
    }

    fn schedule_acknowledged_hide(&self, app: AppHandle, request_id: u64, delay: Duration) {
        let manager = self.clone();
        thread::spawn(move || {
            let mut clock = DismissalClock::new(Instant::now(), delay);
            loop {
                if manager.shown_request.load(Ordering::Acquire) != request_id {
                    return;
                }
                if clock.should_hide(Instant::now(), manager.hovered.load(Ordering::Acquire)) {
                    let Some(window) = app.get_webview_window("overlay") else {
                        let _ = manager.hide_if_acknowledged(&app, request_id);
                        return;
                    };
                    if window
                        .emit("overlay-dismiss-requested", request_id)
                        .is_err()
                    {
                        let _ = manager.hide_if_acknowledged(&app, request_id);
                        return;
                    }

                    let completion_deadline = Instant::now() + OVERLAY_DISMISS_COMPLETION_TIMEOUT;
                    loop {
                        let now = Instant::now();
                        match dismissal_handshake_state(
                            manager.shown_request.load(Ordering::Acquire),
                            request_id,
                            manager.hovered.load(Ordering::Acquire),
                            now,
                            completion_deadline,
                        ) {
                            DismissalHandshakeState::Waiting => {
                                thread::sleep(Duration::from_millis(16));
                            }
                            DismissalHandshakeState::CancelledByHover => {
                                // Renewed reading intent cancels only automatic
                                // dismissal and reuses the existing hover clock.
                                clock.should_hide(now, true);
                                break;
                            }
                            DismissalHandshakeState::TimedOut => {
                                let _ = manager.hide_if_acknowledged(&app, request_id);
                                return;
                            }
                            DismissalHandshakeState::Superseded => return,
                        }
                    }
                }
                thread::sleep(Duration::from_millis(50));
            }
        });
    }

    fn schedule_delivery_watchdog(&self, app: AppHandle, request_id: u64) {
        let manager = self.clone();
        thread::spawn(move || {
            let started = Instant::now();
            let retry_offsets = [
                Duration::from_millis(50),
                Duration::from_millis(150),
                Duration::from_millis(300),
            ];
            for offset in retry_offsets {
                sleep_until(started, offset);
                if !manager.should_retry_delivery(request_id) {
                    return;
                }
                if let (Some(window), Some(payload)) = (
                    app.get_webview_window("overlay"),
                    manager.latest_capture.get(),
                ) {
                    let _ = window.emit("capture-result", payload);
                }
            }

            sleep_until(started, OVERLAY_DELIVERY_TIMEOUT);
            match manager.hide_if_unacknowledged(&app, request_id) {
                Ok(true) => warn!(
                    application = "Translay",
                    capture_method = "overlay-delivery",
                    text_length = 0,
                    elapsed_ms = OVERLAY_DELIVERY_TIMEOUT.as_millis(),
                    error_code = "OVERLAY_DELIVERY_TIMEOUT",
                    request_id,
                    "overlay delivery acknowledgement timed out"
                ),
                Ok(false) => {}
                Err(_) => warn!(
                    application = "Translay",
                    capture_method = "overlay-delivery",
                    text_length = 0,
                    elapsed_ms = OVERLAY_DELIVERY_TIMEOUT.as_millis(),
                    error_code = "OVERLAY_DELIVERY_TIMEOUT_HIDE_FAILED",
                    request_id,
                    "overlay delivery timeout hide failed"
                ),
            }
        });
    }

    fn should_retry_delivery(&self, request_id: u64) -> bool {
        self.shown_request.load(Ordering::Acquire) == request_id
            && self.latest_capture.is_current_unacknowledged(request_id)
    }
}

fn apply_overlay_layout(
    hwnd: HWND,
    payload: &CapturePayload,
    layout: OverlayLayoutContext,
) -> Result<(), String> {
    apply_overlay_layout_with_height(hwnd, payload, layout, None)
}

fn apply_overlay_layout_with_height(
    hwnd: HWND,
    payload: &CapturePayload,
    layout: OverlayLayoutContext,
    height_override: Option<i32>,
) -> Result<(), String> {
    let work_width_logical =
        ((layout.work_area.width() as i64 * 96) / layout.dpi.max(1) as i64) as i32;
    let maximum_width = (work_width_logical - 16).max(OVERLAY_MIN_WIDTH);
    let logical_size = overlay_logical_size(payload, maximum_width);
    let width = scale_for_dpi(logical_size.width, layout.dpi);
    let height = scale_for_dpi(height_override.unwrap_or(logical_size.height), layout.dpi);
    let gap = scale_for_dpi(OVERLAY_GAP, layout.dpi);
    let position = calculate_overlay_position(layout.anchor, layout.work_area, width, height, gap);

    // SAFETY: all coordinates are physical screen pixels and hwnd is the live
    // overlay window. The window remains non-activating while it is resized.
    unsafe {
        SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            position.x,
            position.y,
            width,
            height,
            SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_ASYNCWINDOWPOS | SWP_SHOWWINDOW,
        )
        .map_err(|error| format!("SetWindowPos 更新浮层布局失败：{error}"))
    }?;

    // SWP_ASYNCWINDOWPOS returns when the resize has only been queued. If the
    // result event is emitted immediately, WebView2 can consume the CSS reveal
    // while the old native surface is still being presented.
    wait_for_window_bounds(
        hwnd,
        position.x,
        position.y,
        width,
        height,
        Duration::from_millis(120),
    );
    Ok(())
}

fn sleep_until(started: Instant, offset: Duration) {
    if let Some(remaining) = offset.checked_sub(started.elapsed()) {
        thread::sleep(remaining);
    }
}

fn wait_for_visibility(hwnd: HWND, expected: bool, timeout: Duration) -> bool {
    let started = Instant::now();
    loop {
        // SAFETY: hwnd is a live Tauri top-level window.
        if unsafe { IsWindowVisible(hwnd).as_bool() } == expected {
            return true;
        }
        if started.elapsed() >= timeout {
            return false;
        }
        thread::sleep(Duration::from_millis(2));
    }
}

fn wait_for_window_bounds(
    hwnd: HWND,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    timeout: Duration,
) -> bool {
    let started = Instant::now();
    loop {
        let mut bounds = RECT::default();
        // SAFETY: hwnd is a live Tauri top-level window and bounds is a valid
        // output buffer for the duration of the call.
        if unsafe { GetWindowRect(hwnd, &mut bounds) }.is_ok()
            && bounds.left == x
            && bounds.top == y
            && bounds.right - bounds.left == width
            && bounds.bottom - bounds.top == height
        {
            return true;
        }
        if started.elapsed() >= timeout {
            return false;
        }
        thread::sleep(Duration::from_millis(2));
    }
}

fn overlay_hwnd(app: &AppHandle) -> Result<HWND, String> {
    app.get_webview_window("overlay")
        .ok_or_else(|| "找不到预创建的 overlay 窗口".to_owned())?
        .hwnd()
        .map_err(|error| error.to_string())
}

pub(crate) fn monitor_metrics(
    anchor: ScreenRect,
    source_hwnd: HWND,
) -> Result<(ScreenRect, u32), String> {
    let native_rect = RECT {
        left: anchor.left,
        top: anchor.top,
        right: anchor.right.max(anchor.left + 1),
        bottom: anchor.bottom.max(anchor.top + 1),
    };
    // SAFETY: native_rect and MONITORINFO follow their API contracts.
    unsafe {
        let monitor = MonitorFromRect(&native_rect, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return Err("GetMonitorInfoW 无法取得显示器工作区域".to_owned());
        }
        let mut dpi_x = 0_u32;
        let mut dpi_y = 0_u32;
        let dpi = if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y).is_ok()
            && dpi_x > 0
        {
            dpi_x
        } else {
            GetDpiForWindow(source_hwnd)
        };
        let work = info.rcWork;
        Ok((
            ScreenRect {
                left: work.left,
                top: work.top,
                right: work.right,
                bottom: work.bottom,
            },
            if dpi == 0 { 96 } else { dpi },
        ))
    }
}

pub fn cursor_anchor() -> ScreenRect {
    let mut point = Default::default();
    // SAFETY: point is a valid output pointer.
    if unsafe { GetCursorPos(&mut point) }.is_ok() {
        ScreenRect {
            left: point.x,
            top: point.y,
            right: point.x + 1,
            bottom: point.y + 1,
        }
    } else {
        ScreenRect {
            left: 0,
            top: 0,
            right: 1,
            bottom: 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::CapturePhase;

    fn payload(request_id: u64, text: &str) -> CapturePayload {
        CapturePayload {
            request_id,
            phase: CapturePhase::Translated,
            success: true,
            text: text.to_owned(),
            application_name: "test".to_owned(),
            process_id: 1,
            capture_method: "UIA:TextPattern".to_owned(),
            elapsed_ms: 76,
            selection_rect: None,
            error_code: None,
            error_message: None,
            focus_preserved: true,
            clipboard_restored: None,
            warning_code: None,
            language_profile: Some(crate::translation::LanguageProfile::analyze(text)),
            translation_mode: Some(crate::translation::TranslationMode::Conversational),
        }
    }

    #[test]
    fn payload_is_staged_before_native_show_work() {
        let store = LatestCaptureStore::default();
        let manager = OverlayManager::new(store.clone());
        manager.begin_request(21);

        assert!(manager.latest_capture.store(payload(21, "Edge selection")));
        assert_eq!(store.get().unwrap().text, "Edge selection");
    }

    #[test]
    fn completed_hide_clears_staged_payload() {
        let store = LatestCaptureStore::default();
        let manager = OverlayManager::new(store.clone());
        manager.begin_request(22);
        assert!(manager.latest_capture.store(payload(22, "private text")));

        manager.finish_hide(22);
        assert!(store.get().is_none());
    }

    #[test]
    fn delivery_retries_stop_after_acknowledgement() {
        let store = LatestCaptureStore::default();
        let manager = OverlayManager::new(store.clone());
        manager.begin_request(23);
        assert!(manager.latest_capture.store(payload(23, "delivered")));
        manager.shown_request.store(23, Ordering::Release);
        assert!(manager.latest_capture.mark_shown(23));

        assert!(manager.should_retry_delivery(23));
        assert_eq!(store.acknowledge(23), AckStatus::Accepted);
        assert!(!manager.should_retry_delivery(23));
    }

    #[test]
    fn new_generation_invalidates_old_timers_and_ack() {
        let store = LatestCaptureStore::default();
        let manager = OverlayManager::new(store.clone());
        manager.begin_request(24);
        assert!(manager.latest_capture.store(payload(24, "old")));
        manager.shown_request.store(24, Ordering::Release);
        assert!(manager.latest_capture.mark_shown(24));

        manager.begin_request(25);
        assert!(!manager.latest_capture.is_acknowledged(24));
        assert!(!manager.should_retry_delivery(24));
    }

    #[test]
    fn safety_timeout_cleanup_removes_unacknowledged_payload() {
        let store = LatestCaptureStore::default();
        let manager = OverlayManager::new(store.clone());
        manager.begin_request(26);
        assert!(
            manager
                .latest_capture
                .store(payload(26, "never acknowledged"))
        );
        manager.shown_request.store(26, Ordering::Release);
        assert!(manager.latest_capture.mark_shown(26));
        assert!(manager.latest_capture.is_current_unacknowledged(26));

        manager.finish_hide(26);
        assert!(store.get().is_none());
    }
}
